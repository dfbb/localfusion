# Task 07: `ModelUsage` +3 fields; update every construction site

**Files:**
- Modify: `src/unified.rs` (the `ModelUsage` struct + its test helper)
- Modify: every `ModelUsage { ... }` construction site (listed below)

**Interfaces:**
- Consumes: nothing new.
- Produces: `ModelUsage` gains `cache_read_tokens: u64`, `cache_write_tokens: u64`, `billable_input_tokens: u64`. Cost (Task 09) and connectors (Task 08) consume these. This task adds the fields and sets them to a **correct passthrough default** at every site (`cache_read_tokens: 0, cache_write_tokens: 0, billable_input_tokens: <that site's input_tokens>`); Task 08 then refines the three real connectors to extract actual cache values.

**Context:** `billable_input_tokens` has NO meaningful default (a 0 would zero out input cost), so it must be set explicitly everywhere. The simplest correct value at every site is "whatever that site already passes as `input_tokens`" — for sites with no cache split this is exactly right and Task 08 overrides only the three connector sites that have real cache data. Construction sites (from `grep -rn "ModelUsage {" src`):
`src/unified.rs:214` (test helper), `src/pipeline.rs:106` (test helper), `src/connector/sse.rs:234`, `src/connector/anthropic.rs:100` & `:216`, `src/connector/chat.rs:122` & `:235`, `src/connector/responses.rs:97` & `:201`, `src/admin/api.rs:1249` (test), `src/strategy/testutil.rs:22` & `:39`, `src/strategy/failover.rs:22`, `src/strategy/synthesize.rs:131` & `:196`, `src/strategy/mod.rs:135`.

- [ ] **Step 1: Add the three fields to `ModelUsage`**

In `src/unified.rs`, the `ModelUsage` struct becomes:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model_id: String,
    pub role: CallRole,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Non-cached input tokens, used for cost only. MUST be set explicitly at every
    /// construction site (no meaningful default: 0 would zero out input cost).
    pub billable_input_tokens: u64,
    /// Tokens served from cache (cheaper input). Defaults to 0 where there's no cache info.
    pub cache_read_tokens: u64,
    /// Tokens written to cache (Anthropic cache creation). Defaults to 0.
    pub cache_write_tokens: u64,
    pub cost: f64,
    pub status: CallStatus,
    pub estimated: bool,
    pub latency_secs: f64,
}
```

- [ ] **Step 2: Update every construction site to set the three fields**

At EACH site listed above, add the three fields. The pattern: set `billable_input_tokens` to the same value that site sets `input_tokens` to, and the two cache fields to 0. Concrete examples:

`src/unified.rs:214` (test helper `usage`) — find the `input_tokens: N, output_tokens: M,` lines and add after them:
```rust
            billable_input_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
```
(use the helper's actual `input_tokens` value for `billable_input_tokens`; if the helper takes params, mirror the input param.)

`src/pipeline.rs:106` (test helper `mu(model, inn, out, status)`):
```rust
            input_tokens: inn,
            output_tokens: out,
            billable_input_tokens: inn,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
```

For each connector site (`sse.rs:234`, `anthropic.rs:100`/`:216`, `chat.rs:122`/`:235`, `responses.rs:97`/`:201`), where the existing literal sets `input_tokens: input` (non-stream) or `input_tokens: self.input_tokens` (SSE finish), add immediately after that line:
```rust
        billable_input_tokens: input,        // non-stream sites
        cache_read_tokens: 0,
        cache_write_tokens: 0,
```
or for SSE finish sites:
```rust
            billable_input_tokens: self.input_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
```
(Task 08 refines these connector values; here they get the passthrough default so the codebase compiles.)

For `strategy/*` and `admin/api.rs` test sites that use inline `input_tokens: in_tok` / `input_tokens: 0` etc., mirror the same: `billable_input_tokens: <same as input_tokens>, cache_read_tokens: 0, cache_write_tokens: 0`.

> Tip: after editing, `grep -rn "ModelUsage {" src` and confirm each block now contains `billable_input_tokens`. The compiler will also flag any missed site as "missing field".

- [ ] **Step 3: Compile to find any missed sites**

Run: `cargo build 2>&1 | grep -A3 "missing field" || echo "no missing fields"`
Expected: "no missing fields" (every site updated). If any appear, add the three fields there too.

- [ ] **Step 4: Run the full lib test suite**

Run: `cargo test --lib`
Expected: PASS. (No behavior changed yet — `billable_input_tokens == input_tokens` everywhere, cache fields 0, and cost still uses only `input_tokens` until Task 09.)

- [ ] **Step 5: Clippy**

Run: `cargo clippy --all-targets`
Expected: no new warnings.

- [ ] **Step 6: Commit**

```bash
git add src/unified.rs src/pipeline.rs src/connector src/strategy src/admin/api.rs
git commit -m "feat(unified): add billable_input/cache token fields to ModelUsage (passthrough defaults)"
```
