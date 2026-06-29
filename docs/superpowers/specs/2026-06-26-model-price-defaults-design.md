# Model Price Defaults from litellm — Design

**Date:** 2026-06-26
**Status:** Approved (design); pending written-spec review

## Overview

Real-model prices (input / output / cache-read / cache-write, in USD per
million tokens) gain a default source derived from litellm's
`model_prices_and_context_window.json`. A snapshot is bundled into the binary
at build time, refreshed daily from GitHub at runtime, and fuzzy-matched when a
model is added so its prices are pre-filled. The real-models admin UI displays
and edits all four prices. Cache-read/cache-write prices participate in cost
calculation, which requires extracting cache token counts from upstream usage.

This builds on existing infrastructure: a `prices` table + `src/db/prices.rs`
already exist (`model_id, price_in, price_out, updated_at`), cost is computed in
`src/pipeline.rs::cost_for` (USD per million tokens, `/1e6`), and a read-only
dashboard price view already consumes `GET /admin/api/stats/prices`.

---

## Goals

- Bundle a litellm price snapshot into the binary at build time (committed
  snapshot + rust-embed). No network dependency at build time.
- Refresh the snapshot daily from GitHub at runtime into a dedicated DB table.
- On model add, fuzzy-match the model name against the snapshot and pre-fill the
  four prices.
- Show and edit input / output / cache-read / cache-write prices per model in
  the real-models UI.
- Cache prices participate in cost calculation (requires upstream usage cache
  token extraction across all three connectors).
- A user's manual price edit is authoritative: the daily refresh never touches
  per-model prices.

## Non-Goals (YAGNI)

- Fetching litellm data at build time (snapshot is committed; a separate manual
  script updates it).
- Retroactively updating already-added models' prices when the snapshot
  refreshes.
- Editing the default snapshot from the UI (it is a read-through source, not
  user-managed).
- Context-window / max-token defaults from litellm (this spec is prices only;
  `max_input_tokens` / `default_max_tokens` already handled elsewhere via the
  model `extra` field).

---

## Architecture

### Two tables, separated responsibilities

**1. `price_defaults` (NEW) — litellm snapshot**

```sql
CREATE TABLE IF NOT EXISTS price_defaults (
  model_key   TEXT PRIMARY KEY,  -- litellm model name, e.g. claude-opus-4-8-20260527
  price_in    REAL NOT NULL,     -- USD per million tokens (litellm per-token x 1e6)
  price_out   REAL NOT NULL,
  cache_read  REAL NOT NULL,     -- 0 when litellm omits it
  cache_write REAL NOT NULL,     -- 0 when litellm omits it
  updated_at  INTEGER NOT NULL   -- Unix seconds of the snapshot write
);
```

- Build embeds the committed litellm JSON snapshot into the binary via
  rust-embed (same mechanism as the embedded frontend assets).
- On startup: if `price_defaults` is empty, initialize it from the embedded
  snapshot.
- Daily refresh: fetch the latest JSON from GitHub raw, and on success rewrite
  the whole table in one transaction; on failure keep the current contents.
- This table is **only** the price source for adding a model. It does not
  participate in cost calculation and does not retroactively change
  already-added models.

**2. `prices` (EXISTING, +2 columns) — per-model actual prices**

```sql
-- existing: model_id TEXT PRIMARY KEY, price_in REAL, price_out REAL, updated_at INTEGER
ALTER TABLE prices ADD COLUMN cache_read  REAL NOT NULL DEFAULT 0;
ALTER TABLE prices ADD COLUMN cache_write REAL NOT NULL DEFAULT 0;
```

- `model_id` equals `models.id`.
- Read by cost calculation (`pipeline.rs`).
- On model add, pre-filled by fuzzy-matching `price_defaults`; editable in the
  UI; **never touched by the daily refresh**.

> Schema note: the project creates tables via `CREATE TABLE IF NOT EXISTS` in
> `src/db/schema.rs`. The two new `prices` columns are added there idempotently
> (a guarded `ALTER TABLE ... ADD COLUMN` that ignores "duplicate column"
> errors, or equivalent), so existing databases gain the columns on next open.
> The implementation plan defines the exact migration approach against the
> existing schema-init code.

### Data flow

```
build:    committed litellm JSON snapshot --rust-embed--> binary
startup:  price_defaults empty? -> initialize from embedded snapshot
daily:    GitHub raw JSON -> success: rewrite price_defaults / failure: keep current
add model: model field --fuzzy match--> price_defaults hit -> insert into prices (model_id = new model id)
edit:     user edits the 4 prices in prices -> overwrite
billing:  prices[model_id] 4 prices x per-class token counts -> cost
```

---

## Fuzzy Matching

When a model is added, its `model` field (the upstream real model name) is
matched against `price_defaults.model_key`. Comparison is case-insensitive
(both sides lowercased). Tried in order, stop at first hit:

1. **Exact:** `model` equals a `model_key` (e.g. `gpt-4o` -> `gpt-4o`).
2. **Normalized exact:** replace `.` with `-` in `model`
   (e.g. `claude-opus-4.8` -> `claude-opus-4-8`), then exact match again.
3. **Substring (contains):** the normalized string is a substring of a
   `model_key` (e.g. `claude-opus-4-8` matches `claude-opus-4-8-20260527`).
   When multiple keys contain it:
   - pick the **shortest** `model_key` (least redundant suffix, closest to the
     search term);
   - if still tied, pick the **lexicographically greatest** (litellm's larger
     date suffix is usually newer).
4. **No hit:** do not write a price row. The four price fields are left empty
   (UI shows empty/0); the user fills them manually.

`defaults_match(model) -> Option<PriceRow>` implements this and is unit-tested
across all four cases.

---

## Daily Refresh

Follows the existing background-task pattern (`src/probe.rs::spawn_probe_loop`:
`tokio::spawn` + `tokio::time::interval` + `tokio::sync::watch` shutdown +
`tokio::select!`). A `spawn_price_refresh_loop` is started in `src/main.rs`
alongside the probe loop, sharing the same shutdown channel.

- Period: **24 hours**, and it **runs once immediately at startup** (satisfying
  "update a latest version after launch"), then every day.
- One refresh: GET the litellm raw JSON via the existing reqwest client ->
  parse -> rewrite `price_defaults` in a single transaction (unit conversion
  `x1e6`, missing fields recorded as 0) -> set `updated_at`.
- Failure handling: on network or parse failure, log one `warn` and keep the
  existing `price_defaults` (embedded snapshot or last successful version). The
  service is unaffected; the next cycle retries.
- Fetch URL (raw form, not the blob page):
  `https://raw.githubusercontent.com/BerriAI/litellm/litellm_internal_staging/model_prices_and_context_window.json`

The refresh body is factored into a testable pure function
(JSON string -> parsed rows -> table write) so HTTP success/failure-fallback can
be tested with wiremock. The interval loop itself is not unit-tested (consistent
with the probe loop).

---

## Cache Token Extraction & Cost

### Usage struct

`ModelUsage` (`src/unified.rs`) gains three fields, defaulting to 0:

```rust
pub cache_read_tokens: u64,       // tokens served from cache (cheaper input)
pub cache_write_tokens: u64,      // tokens written to cache (Anthropic only)
pub billable_input_tokens: u64,   // non-cached input tokens, for cost only
```

`input_tokens` keeps its existing meaning and is echoed to the client
**verbatim** from the upstream — no provider's reported token counts change.
`billable_input_tokens` exists purely for the cost formula: it is the non-cached
portion of input, computed per connector from that provider's semantics. The
three billed classes (`billable_input_tokens`, `cache_read_tokens`,
`cache_write_tokens`) are mutually disjoint, so no token is billed twice.

### Per-connector extraction

Each connector extracts cache tokens and computes `billable_input_tokens` when
parsing usage — in both the non-streaming response path and the SSE
trailing-usage path. Missing fields record 0 (backward compatible: when a
connector reports no cache info, `billable_input_tokens == input_tokens` and
the two cache counts are 0, so cost is unchanged from today).

- **anthropic** (`src/connector/anthropic.rs`): Anthropic reports the three
  counts as disjoint — `input_tokens` already excludes cached tokens. Map:
  `cache_read_tokens = usage.cache_read_input_tokens`,
  `cache_write_tokens = usage.cache_creation_input_tokens`,
  `billable_input_tokens = input_tokens` (the reported `input_tokens`, which is
  already non-cached). `input_tokens` (echoed) unchanged.
- **chat / OpenAI** (`src/connector/chat.rs`): OpenAI's `prompt_tokens`
  **includes** `prompt_tokens_details.cached_tokens` (cached is a subset).
  `input_tokens = prompt_tokens` (echoed verbatim);
  `cache_read_tokens = cached_tokens`;
  `billable_input_tokens = prompt_tokens - cached_tokens` (clamped at 0);
  OpenAI has no cache-write billing -> `cache_write_tokens = 0`.
- **responses** (`src/connector/responses.rs`): same as chat —
  `input_tokens = upstream input_tokens` (echoed verbatim);
  `cache_read_tokens = input_tokens_details.cached_tokens`;
  `billable_input_tokens = input_tokens - cached_tokens` (clamped at 0);
  `cache_write_tokens = 0`.

The implementation plan adds a one-line comment at each extraction site stating
which provider convention it follows, and the connector unit tests assert both
the echoed `input_tokens` (unchanged from upstream) and `billable_input_tokens`
(e.g. OpenAI `prompt_tokens=100`, `cached_tokens=30` yields echoed
`input_tokens=100`, `billable_input_tokens=70`, `cache_read_tokens=30`).

### Cost formula

`src/pipeline.rs::cost_for` reads the four `prices` fields and bills the three
disjoint input classes plus output:

```
cost = price_in     * billable_input_tokens / 1e6
     + cache_read   * cache_read_tokens      / 1e6
     + cache_write  * cache_write_tokens     / 1e6
     + price_out    * output_tokens          / 1e6
```

**No double-billing:** `billable_input_tokens`, `cache_read_tokens`, and
`cache_write_tokens` are mutually disjoint by construction, so each term bills a
distinct set of tokens. Client-echoed `input_tokens` / `total_tokens` are
untouched and remain faithful to the upstream.

### Usage statistics

`src/pipeline.rs::write_stats` currently aggregates `input_tokens` and
`output_tokens` into the three usage dimensions (`usage_hourly` rows for real /
virtual / total) and into `request_log.total_tokens` (as `agg_in + agg_out`).
If left unchanged, Anthropic's cache tokens would silently vanish from the
totals (its `input_tokens` excludes them) while OpenAI's would not — an
inconsistency, and a loss of billed volume from the stats.

Resolution (no usage-schema change): the input dimension records the
**uniform true input total**

```
stat_input = billable_input_tokens + cache_read_tokens + cache_write_tokens
```

For OpenAI/responses this equals the echoed `input_tokens` (70 + 30 + 0 = 100);
for Anthropic it equals `input_tokens + cache_read + cache_write` (its
`input_tokens` is already non-cached). So `write_stats` changes `agg_in +=
c.input_tokens` to `agg_in += c.billable_input_tokens + c.cache_read_tokens +
c.cache_write_tokens`. `usage_hourly` / `request_log` schemas are unchanged;
cache tokens are folded into the input dimension rather than stored separately
(YAGNI — no per-class cache breakdown in the dashboard). `request_log.total_tokens`
remains `agg_in + agg_out` and is now complete. The `UsageDelta.input_tokens`
for the per-model "real" row uses the same folded value.

---

## Backend API

- **Add model** (`POST /admin/api/models`): the request body gains four
  **optional** price fields (`price_in`, `price_out`, `cache_read`,
  `cache_write`, USD per million tokens, non-negative). After
  `upsert_from_body` successfully inserts the model, the price row for that
  `model_id` is resolved with this precedence:
  1. If **any** of the four price fields is present in the request, use the
     provided values (absent ones default to 0) and write them to `prices`.
     User-supplied prices take priority — fuzzy match is **not** run.
  2. Otherwise (no price fields at all in the request), run
     `defaults_match(model)`; on a hit, insert the matched four prices; on no
     hit, create no price row.
  This makes the shared add/edit dialog's price inputs meaningful on add: a
  hand-filled price is honored and never overwritten by a default.
- **Edit prices** (NEW `PUT /admin/api/models/:id/prices`): body
  `{ price_in, price_out, cache_read, cache_write }` (USD per million tokens,
  non-negative), upserted into `prices`.
- **Read prices:** the frontend reuses the existing
  `GET /admin/api/stats/prices` (returns all price rows) and merges by
  `model_id` on the models page — smaller backend change than augmenting the
  model list response.
- **Delete model:** does NOT cascade-delete the `prices` row (harmless to keep;
  re-adding a same-named model reuses it).

`src/db/prices.rs` is extended:

- `PriceRow` gains `cache_read` and `cache_write`; `price_upsert` writes all
  four.
- New `price_defaults` methods: `defaults_replace_all(rows)` (single-transaction
  full-table rewrite) and `defaults_match(model) -> Option<PriceRow>` (the
  4-step fuzzy match above).

---

## Frontend — Real-Models UI

1. **Add/edit dialog** (`web/src/features/models/components/models-action-dialog.tsx`):
   a price block with four numeric inputs — input / output / cache-read /
   cache-write price (labeled USD per million tokens).
   - **Add:** the four fields start empty. If the user leaves them empty, the
     backend fuzzy-matches and fills defaults on save. If the user fills any of
     them, those values are sent in the `POST /admin/api/models` body and take
     priority over fuzzy match (per the Backend API precedence). Either way the
     resulting prices are visible on the next edit / in the list; the dialog
     does not pre-fetch defaults before save.
   - **Edit:** the four fields are back-filled from the existing `prices` row;
     on change the dialog calls `PUT /admin/api/models/:id/prices`.
   - The form schema (`web/src/features/models/data/schema.ts`) gains four
     optional non-negative number fields.
2. **List column** (`web/src/features/models/components/models-columns.tsx`):
   a price column showing prices compactly (e.g. in/out as the primary pair,
   cache prices omitted or in a tooltip to avoid an over-wide column). The
   exact presentation is decided at implementation time against table width;
   the spec only requires prices be visible in the list.
3. **Data fetch:** the models page reuses `GET /admin/api/stats/prices`, merges
   by `model_id` with the model list, and invalidates that query after an edit.
4. **i18n:** all new copy (four price labels, the unit note, the column header)
   follows the established i18n convention via `t()`, with new `models.*` keys
   added to both `zh.json` and `en.json`.

---

## Testing

The frontend has no unit-test framework (out of scope). Backend uses Rust unit
tests + wiremock, consistent with the codebase.

- **Fuzzy match (`defaults_match`)**: exact; `.`->`-` normalization; substring
  contains; multi-hit shortest-then-lexicographically-greatest; no-hit returns
  None.
- **Unit conversion**: litellm per-token `x1e6` -> per-million; missing fields
  recorded as 0.
- **DB**: `price_defaults` full-table rewrite transaction; `prices` four-field
  upsert.
- **Cost formula**: `cost_for` with `billable_input_tokens` +
  cache-read/cache-write tokens (construct a `ModelUsage` and assert the
  computed cost); include the no-cache case where `billable_input_tokens ==
  input_tokens` and cost equals today's formula.
- **Usage stat folding**: `write_stats` aggregates the input dimension as
  `billable_input_tokens + cache_read_tokens + cache_write_tokens`; assert the
  recorded `usage_hourly` input and `request_log.total_tokens` for an
  Anthropic-style `ModelUsage` (input excludes cache) and an OpenAI-style one
  (billable + cache = echoed input) both yield the same true total.
- **Add-model price precedence**: `POST /admin/api/models` with explicit price
  fields writes those values and skips fuzzy match; the same POST with no price
  fields falls back to `defaults_match`. (Integration test via the admin API,
  alongside the existing model-create tests.)
- **Connector cache extraction**: for each of the three connectors, feed a
  usage JSON carrying the cache fields and assert echoed `input_tokens`
  (unchanged from upstream), `billable_input_tokens`, `cache_read_tokens`, and
  `cache_write_tokens`; include the field-absent case (cache counts 0,
  `billable_input_tokens == input_tokens`) and the OpenAI subset case
  (`prompt_tokens=100`, `cached_tokens=30` -> echoed 100, billable 70).
- **Daily refresh**: the refresh logic is a testable function
  (JSON string -> parse -> table write); use wiremock for HTTP
  success / failure-fallback. The interval loop itself is not unit-tested
  (consistent with the probe loop).
- **i18n**: `pnpm check:i18n` key parity; frontend `pnpm exec tsc -b`.

---

## Files Changed

| File | Change |
|---|---|
| `build.rs` | Embed (or expose) the committed litellm JSON snapshot |
| `<bundled litellm snapshot JSON>` | New — committed default snapshot (path set in plan; rust-embed source) |
| `src/db/schema.rs` | New `price_defaults` table; `prices` +2 columns (cache_read, cache_write) |
| `src/db/prices.rs` | `PriceRow` +2 fields; `price_upsert` 4 fields; `defaults_replace_all`, `defaults_match` |
| `src/unified.rs` | `ModelUsage` +cache_read_tokens, +cache_write_tokens, +billable_input_tokens |
| `src/connector/chat.rs` | Extract `cached_tokens`, compute billable input (non-stream + SSE) |
| `src/connector/anthropic.rs` | Extract cache_read/cache_creation tokens, set billable input (non-stream + SSE) |
| `src/connector/responses.rs` | Extract `cached_tokens`, compute billable input (non-stream + SSE) |
| `src/pipeline.rs` | `cost_for` bills billable_input + cache-read + cache-write + output; `write_stats` folds cache tokens into the input dimension |
| `src/admin/api.rs` | Add-model: optional price fields (priority over fuzzy match) else `defaults_match` fill; `PUT /admin/api/models/:id/prices` |
| `src/price_refresh.rs` (or similar) | New — refresh function + `spawn_price_refresh_loop` |
| `src/main.rs` | Spawn the daily price-refresh loop; startup snapshot init |
| `web/src/features/models/components/models-action-dialog.tsx` | Four price inputs |
| `web/src/features/models/components/models-columns.tsx` | Price column |
| `web/src/features/models/data/schema.ts` | Four optional price fields |
| `web/src/features/models/components/models-provider.tsx` | Price edit mutation + merge prices by model_id |
| `web/src/i18n/locales/{zh,en}.json` | New `models.*` price keys |
