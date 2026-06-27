use async_trait::async_trait;
use futures::future::join_all;

use super::synthesize::{answer_text_pub, make_text_request, question_text};
use super::{call_member, Strategy, StrategyCtx, StrategyOutput};
use crate::error::FusionError;
use crate::unified::CallRole;

pub struct BestOfN;

/// Build the selection prompt: combine the question and candidate answers into an instruction for the judge to pick the best and fix its flaws
fn selection_prompt(question: &str, answers: &[(String, String)]) -> String {
    let mut p = format!("Question:\n{question}\n\nCandidate solutions:\n");
    for (i, (model, ans)) in answers.iter().enumerate() {
        p.push_str(&format!("\n[Candidate {} from {}]\n{}\n", i + 1, model, ans));
    }
    p.push_str(
        "\nPick the strongest candidate and repair its flaws. \
         Output the final solution, then a line starting with 'Verify by:' \
         describing how to verify it.",
    );
    p
}

#[async_trait]
impl Strategy for BestOfN {
    fn name(&self) -> &str {
        "best-of-n"
    }

    /// Execute the best-of-n strategy:
    /// 1. Call all members in parallel and collect candidate answers
    /// 2. Call the judge to select the best candidate and fix its flaws
    /// Always returns Full (non-streaming)
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError> {
        // Read the required judge parameter
        let judge_id = ctx
            .params
            .get("judge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                FusionError::StrategyError("best-of-n requires params.judge".into())
            })?;

        // Send requests to all members in parallel
        let futs = ctx
            .members
            .iter()
            .map(|m| call_member(m, &ctx.req, CallRole::Member, ctx.recorder));
        let results = join_all(futs).await;

        // Collect successful candidate answers and write them into the trace
        let mut answers: Vec<(String, String)> = Vec::new();
        for (m, r) in ctx.members.iter().zip(results) {
            if let Ok(resp) = r {
                let text = answer_text_pub(&resp);
                if let Some(t) = ctx.trace {
                    let u = resp.calls.first().cloned().unwrap();
                    t.add_member_answer(&m.model_id, &text, &u);
                }
                if !text.trim().is_empty() {
                    answers.push((m.model_id.clone(), text));
                }
            }
        }

        // Return an error if no valid candidates were collected
        if answers.is_empty() {
            return Err(FusionError::AllMembersFailed(
                "best-of-n: no candidates".into(),
            ));
        }

        // Write diversity status into the trace
        if let Some(t) = ctx.trace {
            let status = if answers.len() >= ctx.members.len() {
                "full"
            } else {
                "degraded"
            };
            t.set_status(status);
        }

        // Resolve the judge member, build the selection prompt, and make the call
        let judge_member = ctx.resolver.resolve(judge_id).await?;
        let prompt = selection_prompt(&question_text(&ctx.req), &answers);
        let judge_req = make_text_request(&prompt, ctx.req.max_tokens);
        let judge_resp =
            call_member(&judge_member, &judge_req, CallRole::Judge, ctx.recorder).await?;

        // Write the judge call into the trace
        if let Some(t) = ctx.trace {
            let u = judge_resp.calls.first().cloned().unwrap();
            t.set_judge(&prompt, &answer_text_pub(&judge_resp), &u);
        }

        // best-of-n always returns Full (non-streaming)
        Ok(StrategyOutput::Full(judge_resp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{models::ModelRow, Db};
    use crate::strategy::testutil::{mock_member, simple_req, MockReply};
    use crate::strategy::{synthesize::answer_text_pub, StrategyCtx, StrategyOutput};
    use crate::unified::CallRecorder;

    #[tokio::test]
    async fn selects_and_repairs_via_judge() {
        let db = Db::open_memory().await.unwrap();
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
        let resolver = crate::router::ModelResolver::with_mock(db.clone(), |_id| {
            mock_member(
                "j",
                vec![MockReply::Ok {
                    text: "best".into(),
                    in_tok: 1,
                    out_tok: 1,
                }],
            )
        });
        let recorder = CallRecorder::default();
        let members = vec![
            mock_member(
                "a",
                vec![MockReply::Ok {
                    text: "cand-a".into(),
                    in_tok: 1,
                    out_tok: 1,
                }],
            ),
            mock_member(
                "b",
                vec![MockReply::Ok {
                    text: "cand-b".into(),
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
            trace: None,
        };
        match BestOfN.execute(ctx).await.unwrap() {
            StrategyOutput::Full(r) => assert!(answer_text_pub(&r).contains("best")),
            _ => panic!(),
        }
    }
}
