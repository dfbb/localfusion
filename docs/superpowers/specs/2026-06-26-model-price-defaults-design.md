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

`defaults_match(model: &str) -> Option<PriceValues>` implements this and is
unit-tested across all four cases. It returns a model-id-free price struct
(see below), **not** a `PriceRow` — the snapshot only knows litellm
`model_key`s, not the local model's `id`. The caller is responsible for
attaching the local `model_id`:

```rust
// Model-id-free default prices (all USD per million tokens).
pub struct PriceValues {
    pub price_in: f64,
    pub price_out: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}
```

`PriceRow` (the per-model row, which carries `model_id` and `updated_at`) is
constructed by the caller from a `PriceValues` plus the local `model_id` and
`updated_at = now_secs()`, so a default can never be written under the wrong key
and every write timestamps itself.

---

## Daily Refresh

Follows the existing background-task pattern (`src/probe.rs::spawn_probe_loop`:
`tokio::spawn` + `tokio::time::interval` + `tokio::sync::watch` shutdown +
`tokio::select!`). A `spawn_price_refresh_loop` is started in `src/main.rs`
alongside the probe loop, sharing the same shutdown channel.

- Period: **24 hours**, and it **runs once immediately at startup** (satisfying
  "update a latest version after launch"), then every day.
- One refresh: GET the litellm raw JSON via the existing reqwest client ->
  parse (see field mapping below) -> rewrite `price_defaults` in a single
  transaction -> set `updated_at`.
- Failure handling: on network or parse failure, log one `warn` and keep the
  existing `price_defaults` (embedded snapshot or last successful version). The
  service is unaffected; the next cycle retries.
- Fetch URL (raw form, not the blob page):
  `https://raw.githubusercontent.com/BerriAI/litellm/litellm_internal_staging/model_prices_and_context_window.json`

### litellm JSON parsing & field mapping

The file is a flat JSON object: `{ "<model_key>": { ...fields... }, ... }`
(~2900 entries). Each value's cost fields are **USD per single token**;
`price_defaults` stores **USD per million tokens**, so every parsed value is
multiplied by `1e6`. Exact field mapping (field names verified against the live
file):

| `price_defaults` column | litellm field | Notes |
|---|---|---|
| `price_in` | `input_cost_per_token` | required (see skip rule) |
| `price_out` | `output_cost_per_token` | missing -> 0 |
| `cache_read` | `cache_read_input_token_cost` | missing -> 0 |
| `cache_write` | `cache_creation_input_token_cost` | missing -> 0 (OpenAI models omit it) |

Parsing rules:

- **Skip the `sample_spec` key** — it is a schema-documentation pseudo-entry,
  not a model.
- **Skip entries without `input_cost_per_token`** — ~472 entries price by
  second / character / image / request and have no per-token input cost; they
  cannot map to per-million-token and are not stored. (Log the skipped count at
  debug level, per the no-silent-caps guidance.)
- Each cost field is read as a float and `x1e6`. Only the four mapped fields are
  read; the many tiered/variant fields (`*_above_200k_tokens`, `*_batches`,
  `*_priority`, `*_flex`, audio/image/video variants, DeepSeek's
  `input_cost_per_token_cache_hit`) are intentionally **ignored** — the defaults
  capture standard base pricing only. A future spec may add tier awareness;
  out of scope here.
- `cache_read` / `cache_write` absent -> 0 (correct for models without prompt
  caching). `price_out` absent -> 0.

The refresh body is factored into a testable pure function
(JSON string -> parsed rows -> table write) so HTTP success/failure-fallback can
be tested with wiremock. The interval loop itself is not unit-tested (consistent
with the probe loop).

---

## Cache Token Extraction & Cost

### Usage struct

`ModelUsage` (`src/unified.rs`) gains three fields:

```rust
pub cache_read_tokens: u64,       // tokens served from cache (cheaper input); defaults to 0
pub cache_write_tokens: u64,      // tokens written to cache (Anthropic only); defaults to 0
pub billable_input_tokens: u64,   // non-cached input tokens, for cost only; see below
```

`cache_read_tokens` and `cache_write_tokens` default to 0 (a connector that
reports no cache info leaves them 0). **`billable_input_tokens` does NOT default
to 0** — every connector and every `ModelUsage` constructor MUST set it
explicitly: to `input_tokens` when there is no cache breakdown, or to the
non-cached input amount when there is. Leaving it 0 would zero out the input
cost of ordinary non-cached requests, so the field has no meaningful default and
every construction site (connectors, tests, estimation paths) must assign it.

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

- **Add model** (`POST /admin/api/models`): note `upsert_from_body` is an
  **upsert**, so this endpoint is hit both for genuinely new models and for
  re-submitting an existing one. The price-resolution rules below must NOT
  clobber a model's existing hand-set prices on a repeat POST. The request body
  gains four **optional** price fields (`price_in`, `price_out`, `cache_read`,
  `cache_write`, USD per million tokens, non-negative). After
  `upsert_from_body` succeeds, the price row for that `model_id` is resolved
  with this precedence:
  1. If **any** of the four price fields is present in the request (see
     "Present" below), upsert a `PriceRow` keyed by the local `model_id` from
     the provided values (absent ones default to 0). Explicit prices always
     win, including on a repeat POST (the user is intentionally setting them).
     Fuzzy match is **not** run.
  2. Otherwise (no price fields present), fill defaults **only if the model has
     no existing `prices` row** (`price_get(model_id)` returns `None`): run
     `defaults_match(model)`, and on a hit upsert a `PriceRow` built from the
     returned `PriceValues` + the local `model_id`. If a `prices` row already
     exists, leave it untouched — a repeat POST must never overwrite
     hand-set prices with a default. On no hit, create no price row.

  **`updated_at`:** every price write (explicit add, default fill, and the PUT
  edit below) sets `updated_at` to the current Unix seconds (`now_secs()`), so
  the dashboard's staleness display and the existing schema semantics stay
  meaningful.

  **"Present" definition (avoids empty-input → 0-price bug):** a price field is
  "present" only if it deserializes to a **finite number**. The four fields are
  typed as `Option<f64>` in the request body; `None` (absent / JSON `null`)
  counts as not-present. The backend additionally rejects non-finite values
  (NaN / infinity) and negatives as a 400. So an empty form input must arrive as
  absent/`null`, not `0` or `""` — see the frontend rule below. Precedence is
  decided by "any of the four is present", i.e. any is `Some(finite)`.
- **Edit prices** (NEW `PUT /admin/api/models/:id/prices`): body
  `{ price_in, price_out, cache_read, cache_write }` — **all four required**,
  each a non-negative finite number (USD per million tokens). The handler first
  calls `model_get(id)`; if the model does not exist it returns **404** and
  writes nothing — this prevents creating an orphan price row for a
  non-existent model (`prices` has no FK, so the check is enforced in the
  handler, consistent with the delete-cascade goal of never leaving orphans).
  When the model exists, this is a full replace, not a partial update: the four
  values are upserted into `prices` (with `updated_at = now_secs()`). Clearing a
  price means entering `0` explicitly; the frontend edit form makes all four
  required and does not omit them (see frontend). Missing/`null`/non-finite/
  negative fields are a 400.
- **Read prices:** the frontend reuses the existing
  `GET /admin/api/stats/prices` (returns all price rows) and merges by
  `model_id` on the models page — smaller backend change than augmenting the
  model list response.
- **Delete model** (`DELETE /admin/api/models/:id`): after the existing
  in-use reference check passes, delete the model row and its `prices` row
  **atomically in a single DB transaction** (`pool.begin()` ... `commit()`, the
  pattern already used in `src/db/keys.rs` / `src/db/virtual_models.rs`): a new
  `Db` method (e.g. `model_delete_cascade(id)`) runs both `DELETE FROM models`
  and `DELETE FROM prices WHERE model_id = ?` in one transaction so a mid-way
  failure leaves neither an orphan price row nor a half-deleted model. This
  prevents orphan price rows — otherwise `GET /admin/api/stats/prices` would
  keep returning the deleted model's price and the dashboard price table would
  still show it. (Because `prices.model_id == models.id`, an orphan could only
  ever be "reused" by re-creating the exact same local `id`, not the same
  upstream model name — so retention has no real benefit and a clear downside.)
  The existing `model_delete(id)` may be kept or replaced; the implementation
  plan decides whether to add `model_delete_cascade` or wrap the two deletes in
  the handler — but it MUST be one transaction.

`src/db/prices.rs` is extended:

- `PriceRow` gains `cache_read` and `cache_write`; `price_upsert` writes all
  four and sets `updated_at = now_secs()`. The cascade delete (model + its
  price row in one transaction) lives in `src/db/models.rs` as
  `model_delete_cascade(id)` — see Backend API "Delete model".
- New `price_defaults` methods: `defaults_replace_all(rows)` where each row is a
  `(model_key, PriceValues)` pair (single-transaction full-table rewrite of the
  snapshot) and `defaults_match(model: &str) -> Option<PriceValues>` (the 4-step
  fuzzy match above, returning model-id-free `PriceValues` so the caller
  attaches the local `model_id`).

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
   - **Empty-field serialization rule (critical):** an empty price input must be
     **omitted** from the POST payload, not sent as `""`, `null`, or `0`. The
     submit handler strips any price field whose input is blank/`NaN` before
     building the request body, so "left empty" reaches the backend as absent
     and the fuzzy-match path runs. (Without this, a coerced `0` would read as
     "present" and write a 0 price, suppressing the default — the bug this rule
     prevents.) A field the user typed `0` into is intentional and IS sent.
   - **Edit:** the four fields are back-filled from the existing `prices` row
     (a model with no price row back-fills 0s). Because this is the **shared**
     model dialog, edit-save must persist BOTH the model config and the prices —
     two sequential calls:
     1. `PUT /admin/api/models/:id` with the model fields (the existing call,
        unchanged — connector / base_url / model / api key / extra etc.).
     2. on its success, `PUT /admin/api/models/:id/prices` with the four prices
        (all four **required**; clearing a price means typing `0`).

     **Failure handling:** if the model PUT fails, do not call the price PUT;
     show the error and keep the dialog open. If the model PUT succeeds but the
     price PUT fails, surface a clear partial-save error ("model saved, prices
     failed — retry") and keep the dialog open so the user can retry the price
     save; the model query is invalidated so the saved model fields are
     reflected. On full success, invalidate both the models list and the
     prices query, then close the dialog. (Sequencing model-first means a price
     failure never leaves the model unsaved.)

     The price-required rule applies in edit mode only — the add form keeps its
     optional/omittable behavior.
   - The form schema (`web/src/features/models/data/schema.ts`) models the two
     modes: in **add** mode the four price fields are optional and blank inputs
     normalize to `undefined` (so they are dropped from the POST payload rather
     than coerced to 0); in **edit** mode they are required non-negative numbers
     (`z.coerce.number().nonnegative()`), and the full set of four is sent in
     the PUT body. The exact schema shape (a discriminated/conditional schema,
     or two schemas) is an implementation detail; the requirement is: add omits
     blanks, edit requires all four.
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
- **litellm field mapping & unit conversion**: parse a small fixture object and
  assert `input_cost_per_token`/`output_cost_per_token`/
  `cache_read_input_token_cost`/`cache_creation_input_token_cost` map to the
  four columns `x1e6`; `sample_spec` is skipped; an entry without
  `input_cost_per_token` is skipped; absent cache/output fields -> 0.
- **DB**: `price_defaults` full-table rewrite transaction; `prices` four-field
  upsert sets `updated_at`; `model_delete_cascade` removes the model and its
  price row in one transaction (assert both gone; a model with no price row
  deletes cleanly).
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
  fields writes those values and skips fuzzy match; the same POST with the price
  fields absent/`null` falls back to `defaults_match` **only when no price row
  exists**; a **repeat POST with no price fields against a model that already
  has a (hand-set) price row leaves that row unchanged** (the regression this
  guards); a POST with `price_in: 0` (explicit zero) is treated as present and
  writes 0 (not a fuzzy match). (Integration test via the admin API, alongside
  the existing model-create tests.)
- **PUT prices**: `PUT /admin/api/models/:id/prices` with all four valid numbers
  upserts and stamps `updated_at`; a body missing any field, or with a
  negative/non-finite value, returns 400; a PUT for a **non-existent model id
  returns 404 and writes no row** (no orphan).
- **Delete cascade**: deleting a model removes its `prices` row, and
  `GET /admin/api/stats/prices` no longer returns it.
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
| `src/db/prices.rs` | `PriceRow` +2 fields; `price_upsert` 4 fields + `updated_at`; `defaults_replace_all`, `defaults_match` (-> `PriceValues`) |
| `src/db/models.rs` | `model_delete_cascade(id)` — delete model + its `prices` row in one transaction |
| `src/unified.rs` | `ModelUsage` +cache_read_tokens, +cache_write_tokens, +billable_input_tokens |
| `src/connector/chat.rs` | Extract `cached_tokens`, compute billable input (non-stream + SSE) |
| `src/connector/anthropic.rs` | Extract cache_read/cache_creation tokens, set billable input (non-stream + SSE) |
| `src/connector/responses.rs` | Extract `cached_tokens`, compute billable input (non-stream + SSE) |
| `src/pipeline.rs` | `cost_for` bills billable_input + cache-read + cache-write + output; `write_stats` folds cache tokens into the input dimension |
| `src/admin/api.rs` | Add-model: optional finite-number price fields (override) else default-fill only when no price row exists; `PUT /admin/api/models/:id/prices` (4 required); delete-model uses `model_delete_cascade` |
| `src/price_refresh.rs` (or similar) | New — refresh function + `spawn_price_refresh_loop` |
| `src/main.rs` | Spawn the daily price-refresh loop; startup snapshot init |
| `web/src/features/models/components/models-action-dialog.tsx` | Four price inputs |
| `web/src/features/models/components/models-columns.tsx` | Price column |
| `web/src/features/models/data/schema.ts` | Four optional price fields |
| `web/src/features/models/components/models-provider.tsx` | Edit-save = sequential model PUT then prices PUT (partial-failure handling); merge prices by model_id |
| `web/src/i18n/locales/{zh,en}.json` | New `models.*` price keys |
