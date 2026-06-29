# Task 08: Connector cache-token extraction + billable input

**Files:**
- Modify: `src/connector/anthropic.rs`
- Modify: `src/connector/chat.rs`
- Modify: `src/connector/responses.rs`

**Interfaces:**
- Consumes: `ModelUsage` cache fields (Task 07).
- Produces: each connector sets real `cache_read_tokens`, `cache_write_tokens`, and `billable_input_tokens` from upstream usage, in BOTH the non-streaming parse and the SSE `finish()` path. Cost (Task 09) reads these.

**Context — the uniform invariant:** after extraction, `billable_input_tokens` is the non-cached input, and the three classes are disjoint. `input_tokens` (echoed to clients) is unchanged.
- **Anthropic:** `input_tokens` already excludes cached tokens. So `billable = input_tokens`, `cache_read = usage.cache_read_input_tokens`, `cache_write = usage.cache_creation_input_tokens`.
- **OpenAI chat:** `prompt_tokens` INCLUDES `prompt_tokens_details.cached_tokens`. So `cache_read = cached_tokens`, `billable = prompt_tokens - cached_tokens` (saturating), `cache_write = 0`.
- **Responses:** `input_tokens` INCLUDES `input_tokens_details.cached_tokens`. So `cache_read = cached_tokens`, `billable = input_tokens - cached_tokens` (saturating), `cache_write = 0`.

Each connector has two code paths sharing the same `input`/`self.input_tokens`: a non-streaming parse fn and an SSE state machine whose `finish()` builds the `ModelUsage`. Both must be updated. SSE state structs need new fields to carry cache counts parsed from the usage event.

---

## Anthropic (`src/connector/anthropic.rs`)

- [ ] **Step 1: Non-stream parse — extract cache tokens**

After the existing `let output = json.pointer("/usage/output_tokens")...;` block (around line 92-98), add:

```rust
    let cache_read = json
        .pointer("/usage/cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_write = json
        .pointer("/usage/cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
```

In the `ModelUsage { ... }` literal there (the one Task 07 set to passthrough), set:
```rust
        input_tokens: input,
        output_tokens: output,
        billable_input_tokens: input, // Anthropic input_tokens already excludes cached tokens
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
```

- [ ] **Step 2: SSE state — add cache fields**

In `pub(super) struct AnthropicSseState`, add after `output_tokens: u64,`:
```rust
    cache_read_tokens: u64,
    cache_write_tokens: u64,
```
In `AnthropicSseState::new`, add after `output_tokens: 0,`:
```rust
            cache_read_tokens: 0,
            cache_write_tokens: 0,
```

- [ ] **Step 3: SSE push — capture cache tokens from `message_start`**

Anthropic reports cache tokens in the `message_start` event's `message.usage`. In the `"message_start" =>` arm, after the existing `input_tokens` capture, add:
```rust
                if let Some(c) = evt.pointer("/message/usage/cache_read_input_tokens").and_then(|v| v.as_u64()) {
                    self.cache_read_tokens = c;
                }
                if let Some(c) = evt.pointer("/message/usage/cache_creation_input_tokens").and_then(|v| v.as_u64()) {
                    self.cache_write_tokens = c;
                }
```

- [ ] **Step 4: SSE finish — set the fields on the `ModelUsage`**

In `finish()`'s `ModelUsage { ... }` literal, set:
```rust
            input_tokens: self.input_tokens,
            output_tokens: out_tokens,
            billable_input_tokens: self.input_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
```

---

## OpenAI chat (`src/connector/chat.rs`)

- [ ] **Step 5: Non-stream parse — cached subset, compute billable**

After the existing `let output = json.pointer("/usage/completion_tokens")...;` block, add:
```rust
    let cache_read = json
        .pointer("/usage/prompt_tokens_details/cached_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    // OpenAI prompt_tokens INCLUDES cached_tokens; billable input is the non-cached remainder.
    let billable = input.saturating_sub(cache_read);
```

In the non-stream `ModelUsage { ... }` literal, set:
```rust
        input_tokens: input,
        output_tokens: output,
        billable_input_tokens: billable,
        cache_read_tokens: cache_read,
        cache_write_tokens: 0,
```

- [ ] **Step 6: SSE state — add a cache_read field**

In `pub(super) struct ChatSseState`, add after `output_tokens: u64,`:
```rust
    cache_read_tokens: u64,
```
In `ChatSseState::new`, add after `output_tokens: 0,`:
```rust
            cache_read_tokens: 0,
```

- [ ] **Step 7: SSE push — capture cached_tokens from the usage chunk**

In the `if let Some(u) = chunk.get("usage")` block (where `prompt_tokens`/`completion_tokens` are read), add after setting `self.output_tokens`:
```rust
                self.cache_read_tokens = u
                    .pointer("/prompt_tokens_details/cached_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
```

- [ ] **Step 8: SSE finish — set fields (compute billable)**

In `finish()`, before building the `ModelUsage`, add:
```rust
        let billable = self.input_tokens.saturating_sub(self.cache_read_tokens);
```
In the `ModelUsage { ... }` literal, set:
```rust
            input_tokens: self.input_tokens,
            output_tokens: out_tokens,
            billable_input_tokens: billable,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: 0,
```

---

## Responses (`src/connector/responses.rs`)

- [ ] **Step 9: Non-stream parse — cached subset, compute billable**

After the existing `let output = json.pointer("/usage/output_tokens")...;` block, add:
```rust
    let cache_read = json
        .pointer("/usage/input_tokens_details/cached_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let billable = input.saturating_sub(cache_read);
```
In the non-stream `ModelUsage { ... }` literal, set:
```rust
        input_tokens: input,
        output_tokens: output,
        billable_input_tokens: billable,
        cache_read_tokens: cache_read,
        cache_write_tokens: 0,
```

- [ ] **Step 10: SSE state + push + finish**

In `pub(super) struct ResponsesSseState`, add after `output_tokens: u64,`:
```rust
    cache_read_tokens: u64,
```
In `ResponsesSseState::new`, add after `output_tokens: 0,`:
```rust
            cache_read_tokens: 0,
```
In the `"response.completed" | "response.incomplete" =>` arm, inside the `if let Some(u) = evt.pointer("/response/usage")` block, after setting `self.output_tokens`, add:
```rust
                    self.cache_read_tokens = u
                        .pointer("/input_tokens_details/cached_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
```
In `finish()`, before the `ModelUsage`, add `let billable = self.input_tokens.saturating_sub(self.cache_read_tokens);` and in the literal set:
```rust
            input_tokens: self.input_tokens,
            output_tokens: out_tokens,
            billable_input_tokens: billable,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: 0,
```

---

- [ ] **Step 11: Tests — non-stream extraction for all three connectors**

Add a test to each connector's `mod tests` (find the existing `parse_*` test fn name; these connectors already have non-stream parse tests). Example for chat (`src/connector/chat.rs`), adapt the existing parse-test helper to feed cache details:

```rust
    #[test]
    fn parse_extracts_cached_tokens_and_billable() {
        let json = serde_json::json!({
            "choices":[{"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":100,"completion_tokens":5,"prompt_tokens_details":{"cached_tokens":30}}
        });
        let resp = parse_chat_response(&json, "m");
        let c = &resp.calls[0];
        assert_eq!(c.input_tokens, 100);              // echoed verbatim
        assert_eq!(c.cache_read_tokens, 30);
        assert_eq!(c.billable_input_tokens, 70);      // 100 - 30
        assert_eq!(c.cache_write_tokens, 0);
    }
```

For anthropic, use `parse_anthropic_response`, feed `"usage":{"input_tokens":50,"output_tokens":4,"cache_read_input_tokens":10,"cache_creation_input_tokens":20}` and assert `input_tokens==50`, `billable_input_tokens==50`, `cache_read_tokens==10`, `cache_write_tokens==20`. For responses, use `parse_responses_response`, feed `"usage":{"input_tokens":80,"output_tokens":3,"input_tokens_details":{"cached_tokens":25}}` and assert `input_tokens==80`, `billable_input_tokens==55`, `cache_read_tokens==25`.

> The three non-streaming parse fns are `parse_chat_response`, `parse_anthropic_response`, `parse_responses_response` (signature `(json: &Value, model_id: &str) -> UnifiedResponse`). Match the existing test style in each file (see `parse_response_extracts_text_and_usage` / `parse_response_text_and_usage` / `parse_response_output_text_and_usage`).

- [ ] **Step 12: Run connector tests**

Run: `cargo test --lib connector::`
Expected: PASS (new extraction tests + all existing connector tests).

- [ ] **Step 13: Commit**

```bash
git add src/connector/anthropic.rs src/connector/chat.rs src/connector/responses.rs
git commit -m "feat(connector): extract cache tokens and compute billable input across all three connectors"
```
