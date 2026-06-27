use async_trait::async_trait;
use futures::future::join_all;

use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::*;

pub struct Synthesize;

/// 构建合成 prompt：把问题和各成员答案拼成让 judge 做综合的指令
pub(super) fn synthesis_prompt(question: &str, answers: &[(String, String)]) -> String {
    let mut p = format!("Question:\n{question}\n\nCandidate answers from different models:\n");
    for (i, (model, ans)) in answers.iter().enumerate() {
        p.push_str(&format!("\n[Answer {} from {}]\n{}\n", i + 1, model, ans));
    }
    p.push_str(
        "\nReconcile these into one best answer. \
         Note consensus, contradictions, gaps, and blind spots, \
         then write a single superior response.",
    );
    p
}

/// 从 UnifiedResponse 中提取纯文本内容
fn answer_text(resp: &UnifiedResponse) -> String {
    resp.items
        .iter()
        .find_map(|i| match i {
            Item::Message { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// 公开给 best-of-n 和测试复用的文本提取函数
pub(crate) fn answer_text_pub(r: &UnifiedResponse) -> String {
    answer_text(r)
}

/// 从请求中提取用户问题文本（用于拼合成 prompt）
pub(crate) fn question_text(req: &UnifiedRequest) -> String {
    req.items
        .iter()
        .filter_map(|i| match i {
            Item::Message {
                role: Role::User,
                content,
            } => Some(
                content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 构造一个只含单条用户消息的文本请求（给 judge 调用）
pub(crate) fn make_text_request(prompt: &str, max_tokens: Option<u32>) -> UnifiedRequest {
    UnifiedRequest {
        items: vec![Item::Message {
            role: Role::User,
            content: vec![ContentBlock::Text(prompt.into())],
        }],
        tools: vec![],
        max_tokens,
        temperature: None,
        stream: false,
        raw_extra: serde_json::Value::Null,
    }
}

#[async_trait]
impl Strategy for Synthesize {
    fn name(&self) -> &str {
        "synthesize"
    }

    /// 执行合成策略：
    /// 1. 并行调用所有成员，收集答案
    /// 2. 根据 min_answers/strict 判断 diversity 状态
    /// 3. 调用 judge 模型合成最终回答
    /// 始终返回 Full（非流式）
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        // 读取策略参数
        let min_answers = ctx
            .params
            .get("min_answers")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize;
        let strict = ctx
            .params
            .get("strict")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let judge_id = ctx
            .params
            .get("judge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                FusionError::StrategyError("synthesize requires params.judge".into())
            })?;

        // 并行向所有成员发请求；recorder 会在 call_member 内部记录各自调用
        let futs = ctx
            .members
            .iter()
            .map(|m| call_member(m, &ctx.req, CallRole::Member, ctx.recorder));
        let results = join_all(futs).await;

        // 收集成功的答案，同时写入 trace
        let mut answers: Vec<(String, String)> = Vec::new();
        for (m, r) in ctx.members.iter().zip(results) {
            if let Ok(resp) = r {
                let text = answer_text(&resp);
                if let Some(t) = ctx.trace {
                    // 取响应中第一条调用记录作为 trace 用量；若无则构造零值占位
                    let u = resp.calls.first().cloned().unwrap_or(ModelUsage {
                        model_id: m.model_id.clone(),
                        role: CallRole::Member,
                        input_tokens: 0,
                        output_tokens: 0,
                        cost: 0.0,
                        status: CallStatus::Ok,
                        estimated: true,
                        latency_secs: 0.0,
                    });
                    t.add_member_answer(&m.model_id, &text, &u);
                }
                if !text.trim().is_empty() {
                    answers.push((m.model_id.clone(), text));
                }
            }
        }

        // 没有任何有效答案直接报错
        if answers.is_empty() {
            return Err(FusionError::AllMembersFailed(
                "synthesize: no panel answers".into(),
            ));
        }

        // Diversity 分级：full / degraded / stop
        let status = if answers.len() >= ctx.members.len() {
            "full"
        } else if answers.len() >= min_answers {
            "degraded"
        } else {
            "stop"
        };
        if let Some(t) = ctx.trace {
            t.set_status(status);
        }
        // strict 模式：答案不足 min_answers 时中止
        if status == "stop" && strict {
            return Err(FusionError::StrategyError(format!(
                "synthesize strict: only {} answers",
                answers.len()
            )));
        }

        // 解析 judge 成员，构建合成 prompt 并发起调用
        let judge_member = ctx.resolver.resolve(judge_id).await?;
        let prompt = synthesis_prompt(&question_text(&ctx.req), &answers);
        let judge_req = make_text_request(&prompt, ctx.req.max_tokens);
        let judge_resp = match call_member(&judge_member, &judge_req, CallRole::Judge, ctx.recorder).await {
            Ok(r) => r,
            Err(e) => {
                // judge 失败仍报错，但日志注明 panel 已成功收集答案(已付费工作不丢失，§7.4)
                tracing::warn!(
                    judge = judge_id,
                    panel_answers = answers.len(),
                    error = %e,
                    "synthesize judge 调用失败，但已成功收集 {} 份成员答案(用量已计入统计)",
                    answers.len()
                );
                return Err(e);
            }
        };

        // 把 judge 调用写入 trace
        if let Some(t) = ctx.trace {
            let u = judge_resp.calls.first().cloned().unwrap_or(ModelUsage {
                model_id: judge_id.into(),
                role: CallRole::Judge,
                input_tokens: 0,
                output_tokens: 0,
                cost: 0.0,
                status: CallStatus::Ok,
                estimated: true,
                latency_secs: 0.0,
            });
            t.set_judge(&prompt, &answer_text(&judge_resp), &u);
        }

        // synthesize 始终返回 Full（非流式）
        Ok(StrategyOutput::Full(judge_resp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{models::ModelRow, Db};
    use crate::strategy::testutil::{mock_member, simple_req, MockReply};
    use crate::strategy::{StrategyCtx, StrategyOutput};
    use crate::unified::{CallRecorder, StrategyTrace};

    /// 在内存 DB 中插入 judge 模型行（连接器类型 "chat" 即可，测试会走 mock）
    async fn seed_judge(db: &Db) {
        db.model_upsert(&ModelRow {
            id: "j".into(),
            connector: "chat".into(),
            base_url: "u".into(),
            api_key_enc: None,
            api_key_env: Some("E".into()),
            model: "j".into(),
            anthropic_version: None,
            extra: None,
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn collects_members_and_calls_judge() {
        let db = Db::open_memory().await.unwrap();
        seed_judge(&db).await;
        // 使用 with_mock：judge 解析时直接返回 mock_member
        let resolver = crate::router::ModelResolver::with_mock(db.clone(), |_id| {
            mock_member(
                "j",
                vec![MockReply::Ok {
                    text: "synth".into(),
                    in_tok: 1,
                    out_tok: 1,
                }],
            )
        });
        let recorder = CallRecorder::default();
        let trace = StrategyTrace::default();
        let members = vec![
            mock_member(
                "a",
                vec![MockReply::Ok {
                    text: "ans-a".into(),
                    in_tok: 1,
                    out_tok: 1,
                }],
            ),
            mock_member(
                "b",
                vec![MockReply::Ok {
                    text: "ans-b".into(),
                    in_tok: 1,
                    out_tok: 1,
                }],
            ),
        ];
        let ctx = StrategyCtx {
            req: simple_req(),
            members,
            resolver: &resolver,
            params: serde_json::json!({"judge": "j"}),
            db: &db,
            want_stream: false,
            recorder: &recorder,
            trace: Some(&trace),
        };
        match Synthesize.execute(ctx).await.unwrap() {
            StrategyOutput::Full(r) => assert_eq!(answer_text_pub(&r), "synth"),
            _ => panic!("expected Full"),
        }
        let snap = trace.snapshot();
        // 两个成员的答案都应被收集
        assert_eq!(snap["member_answers"].as_array().unwrap().len(), 2);
        // judge 信息应存在
        assert!(snap["judge"].is_object());
    }

    #[tokio::test]
    async fn no_judge_param_returns_error() {
        let db = Db::open_memory().await.unwrap();
        let resolver = crate::router::ModelResolver::with_mock(db.clone(), |id| {
            mock_member(id, vec![])
        });
        let recorder = CallRecorder::default();
        let ctx = StrategyCtx {
            req: simple_req(),
            members: vec![mock_member(
                "a",
                vec![MockReply::Ok {
                    text: "x".into(),
                    in_tok: 1,
                    out_tok: 1,
                }],
            )],
            resolver: &resolver,
            params: serde_json::json!({}), // 缺少 judge
            db: &db,
            want_stream: false,
            recorder: &recorder,
            trace: None,
        };
        assert!(Synthesize.execute(ctx).await.is_err());
    }

    #[tokio::test]
    async fn all_members_fail_returns_error() {
        let db = Db::open_memory().await.unwrap();
        let resolver = crate::router::ModelResolver::with_mock(db.clone(), |id| {
            mock_member(id, vec![])
        });
        let recorder = CallRecorder::default();
        let ctx = StrategyCtx {
            req: simple_req(),
            members: vec![
                mock_member("a", vec![MockReply::Fail("err".into())]),
                mock_member("b", vec![MockReply::Fail("err".into())]),
            ],
            resolver: &resolver,
            params: serde_json::json!({"judge": "j"}),
            db: &db,
            want_stream: false,
            recorder: &recorder,
            trace: None,
        };
        assert!(Synthesize.execute(ctx).await.is_err());
    }
}
