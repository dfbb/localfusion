use async_trait::async_trait;
use futures::future::join_all;

use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::*;

pub struct Synthesize;

/// Build the synthesis prompt: concatenate the question and member answers into an instruction for the judge to synthesize
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

/// Extract plain text content from a UnifiedResponse
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

/// Public text extraction function reused by best-of-n and tests
pub(crate) fn answer_text_pub(r: &UnifiedResponse) -> String {
    answer_text(r)
}

/// Extract the user question text from a request (used to build the synthesis prompt)
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

/// Construct a request containing a single user text message (for calling the judge)
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

    /// Execute the synthesis strategy:
    /// 1. Call all members in parallel and collect answers
    /// 2. Determine diversity status based on min_answers/strict
    /// 3. Call the judge model to synthesize the final answer
    /// Always returns Full (non-streaming)
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        // Read strategy parameters
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

        // Send requests to all members in parallel; recorder logs each call inside call_member
        let futs = ctx
            .members
            .iter()
            .map(|m| call_member(m, &ctx.req, CallRole::Member, ctx.recorder));
        let results = join_all(futs).await;

        // Collect successful answers and write to trace
        let mut answers: Vec<(String, String)> = Vec::new();
        for (m, r) in ctx.members.iter().zip(results) {
            if let Ok(resp) = r {
                let text = answer_text(&resp);
                if let Some(t) = ctx.trace {
                    // Use the first call record in the response as trace usage; construct zero-value placeholder if none
                    let u = resp.calls.first().cloned().unwrap_or(ModelUsage {
                        model_id: m.model_id.clone(),
                        role: CallRole::Member,
                        input_tokens: 0,
                        output_tokens: 0,
                        billable_input_tokens: 0,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
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

        // No valid answers at all — return error immediately
        if answers.is_empty() {
            return Err(FusionError::AllMembersFailed(
                "synthesize: no panel answers".into(),
            ));
        }

        // Diversity tiers: full / degraded / stop
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
        // Strict mode: abort when answers fall below min_answers
        if status == "stop" && strict {
            return Err(FusionError::StrategyError(format!(
                "synthesize strict: only {} answers",
                answers.len()
            )));
        }

        // Resolve judge member, build synthesis prompt, and make the call
        let judge_member = ctx.resolver.resolve(judge_id).await?;
        let prompt = synthesis_prompt(&question_text(&ctx.req), &answers);
        let judge_req = make_text_request(&prompt, ctx.req.max_tokens);
        let judge_resp = match call_member(&judge_member, &judge_req, CallRole::Judge, ctx.recorder).await {
            Ok(r) => r,
            Err(e) => {
                // judge failed but log that panel answers were successfully collected (paid work is not lost, §7.4)
                tracing::warn!(
                    judge = judge_id,
                    panel_answers = answers.len(),
                    error = %e,
                    "synthesize judge call failed, but {} member answers were successfully collected (usage already counted in stats)",
                    answers.len()
                );
                return Err(e);
            }
        };

        // Write the judge call into trace
        if let Some(t) = ctx.trace {
            let u = judge_resp.calls.first().cloned().unwrap_or(ModelUsage {
                model_id: judge_id.into(),
                role: CallRole::Judge,
                input_tokens: 0,
                output_tokens: 0,
                billable_input_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost: 0.0,
                status: CallStatus::Ok,
                estimated: true,
                latency_secs: 0.0,
            });
            t.set_judge(&prompt, &answer_text(&judge_resp), &u);
        }

        // synthesize always returns Full (non-streaming)
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

    /// Insert the judge model row into the in-memory DB (connector type "chat" suffices; tests will use mock)
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
        // Use with_mock: judge resolution returns mock_member directly
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
        // Both member answers should be collected
        assert_eq!(snap["member_answers"].as_array().unwrap().len(), 2);
        // judge info should be present
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
            params: serde_json::json!({}), // missing judge
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
