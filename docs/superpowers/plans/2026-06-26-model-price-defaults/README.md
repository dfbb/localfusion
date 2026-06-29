# Model Price Defaults (litellm) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. This plan is laid out **one task per file** in this directory; read this overview, then execute `task-01-*.md` … `task-12-*.md` in order.

**Goal:** Give real-model prices (input / output / cache-read / cache-write, USD per million tokens) a litellm-derived default source — bundled at build time, refreshed daily into the DB, fuzzy-matched on model add — and show/edit all four prices in the real-models UI, with cache prices participating in cost.

**Architecture:** Two tables: `price_defaults` (litellm snapshot, refreshed daily, the add-time price source) and the existing `prices` (per-model actual prices, read by cost calc, never touched by refresh). A committed litellm JSON snapshot is embedded via `include_str!` at build time and used to seed `price_defaults` when empty; a daily background loop (mirroring `spawn_probe_loop`) re-fetches it from GitHub raw. Connectors extract cache tokens from upstream usage and compute a `billable_input_tokens` so cost and stats stay faithful without double-billing.

**Tech Stack:** Rust (axum 0.7, sqlx 0.8 SQLite, reqwest 0.12, tokio), wiremock for tests; React 19 + Vite + TanStack Query + react-hook-form + zod + react-i18next (pnpm).

## Global Constraints

- All code comments and docs in **English**; UI strings via i18next `t()` with new `models.*` keys in both `zh.json` and `en.json` (identical key set; `pnpm check:i18n` must pass).
- Prices are stored and displayed as **USD per million tokens** (litellm per-token values × 1e6).
- The four cost classes are mutually disjoint: `billable_input_tokens`, `cache_read_tokens`, `cache_write_tokens`, `output_tokens`. `input_tokens` is echoed to clients **verbatim** from upstream and never altered.
- `billable_input_tokens` has **no meaningful default** — every `ModelUsage` construction site MUST set it explicitly (to `input_tokens` when there's no cache split). `cache_read_tokens`/`cache_write_tokens` default to 0.
- litellm field map (USD/token → ×1e6): `price_in`←`input_cost_per_token`, `price_out`←`output_cost_per_token`, `cache_read`←`cache_read_input_token_cost`, `cache_write`←`cache_creation_input_token_cost`. Skip the `sample_spec` key and any entry without `input_cost_per_token`. Ignore all tiered/variant fields.
- Fuzzy match order (case-insensitive): exact → normalized exact (`.`→`-`) → substring-contains (shortest key wins, ties → lexicographically greatest) → no hit.
- `defaults_match` returns model-id-free `PriceValues`; the caller attaches `model_id` + `updated_at = now_secs()` to build a `PriceRow`. A default can never be written under the wrong key.
- POST /models validates price fields **before** any model write (bad price → 400, no mutation). Default-fill happens **only when no `prices` row exists** for that model_id (repeat POST never clobbers hand-set prices). Explicit price fields override and skip fuzzy match.
- A price field is "present" only if it deserializes to a **finite, non-negative number** (`Option<f64>`; NaN/inf/negative → 400). Frontend omits blank price inputs from the POST payload.
- PUT /models/:id/prices requires **all four** finite non-negative fields; 404 if the model doesn't exist (no orphan rows).
- Delete model removes the model and its `prices` row in **one transaction** (`model_delete_cascade`).
- Daily refresh failure is non-fatal: log one `warn`, keep existing `price_defaults`.
- Surgical changes: match existing code style; touch only what each task names.

## Verification commands

- Rust: `cargo test` (workspace), `cargo clippy --all-targets` (clean), `cargo build`.
- Frontend (run from `web/`): `pnpm check:i18n`, `pnpm exec tsc -b`, `pnpm build`.

## Task index

| # | File | Deliverable |
|---|---|---|
| 01 | `task-01-schema-migration.md` | `price_defaults` table + `prices` cache columns + guarded ALTER migration |
| 02 | `task-02-pricerow-and-methods.md` | `PriceRow`/`PriceValues` structs, `price_upsert` (4 fields), `model_delete_cascade` |
| 03 | `task-03-litellm-parse.md` | litellm JSON → `Vec<(model_key, PriceValues)>` parser + `defaults_replace_all` |
| 04 | `task-04-fuzzy-match.md` | `defaults_match` 4-step fuzzy matcher |
| 05 | `task-05-snapshot-embed.md` | committed litellm snapshot + `include_str!` embed + empty-table seed on startup |
| 06 | `task-06-refresh-loop.md` | `price_refresh` fetch fn + `spawn_price_refresh_loop` + main wiring |
| 07 | `task-07-modelusage-fields.md` | `ModelUsage` +3 fields; update all construction sites |
| 08 | `task-08-connector-cache-tokens.md` | extract cache tokens + billable input in chat/anthropic/responses |
| 09 | `task-09-cost-and-stats.md` | `cost_for` 4-term formula; `write_stats` folds cache into input |
| 10 | `task-10-admin-api.md` | POST price-fill precedence, `PUT /models/:id/prices`, delete cascade |
| 11 | `task-11-frontend-prices.md` | price inputs (add/edit), list column, schema, provider two-call save |
| 12 | `task-12-i18n-keys.md` | `models.*` price keys in zh/en + parity |

Tasks 01→10 are backend and mostly sequential (later tasks consume earlier signatures). Task 11 depends on Task 10's API. Task 12 can be folded into Task 11's review but is split so the i18n key parity is its own gate.
