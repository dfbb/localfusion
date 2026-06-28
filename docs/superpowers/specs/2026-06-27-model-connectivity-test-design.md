# Model Connectivity Test — Design

**Date:** 2026-06-27
**Status:** Approved

## Overview

Add a "Test All" button to the real-models page that probes every configured model for connectivity and latency. Results are shown in a new **Status** column in the table. Results are in-memory only (cleared on page refresh or next test run).

---

## Scope

- New backend endpoint: `POST /admin/api/models/test-all`
- Two frontend changes: `models-primary-buttons.tsx` (button) and `models-columns.tsx` + `models-provider.tsx` (state + column)
- No DB writes, no schema changes

---

## Backend

### `POST /admin/api/models/test-all`

**Auth:** admin token (same middleware as all other admin routes).

**Logic:**

1. Fetch all models via `db.model_list()`.
2. Resolve each model to a `MemberHandle` via `ModelResolver::resolve(&model_id)`.  Models whose key cannot be resolved (e.g. missing env var) are recorded as `ok: false, error: "key unavailable"` without making a network request.
3. For resolvable models, send `probe_request()` (identical to `src/probe.rs`: `"ping"`, `max_tokens: 8`, non-streaming) and record wall-clock latency with `std::time::Instant`.
4. All models run concurrently via `futures::future::join_all`.

**Response — `200 OK`:**

```json
[
  { "id": "gpt-4o",   "ok": true,  "latency_ms": 342 },
  { "id": "claude-3", "ok": false, "error": "upstream 401: Inval…" }
]
```

- `latency_ms`: integer milliseconds, present only when `ok: true`.
- `error`: string, present only when `ok: false`. Truncated to 60 characters using the existing `upstream_error` sanitization path.

**Errors:** If `db.model_list()` fails, return `500`. Individual model failures are captured per-item (never propagate to the top level).

---

## Frontend

### State — `models-provider.tsx`

Add two fields to the provider context and state:

```ts
testing: boolean                          // true while POST in flight
testResults: Map<string, TestResult>      // keyed by model id
```

```ts
type TestResult =
  | { ok: true;  latency_ms: number }
  | { ok: false; error: string }
```

- `testing` starts `false`, set to `true` on button click, back to `false` on response/error.
- `testResults` is cleared to an empty `Map` at the start of each test run, then populated when the response arrives.

### Button — `models-primary-buttons.tsx`

Add a "Test All" button to the right of the existing "New Model" button:

```
[ + New Model ]  [ ⚡ Test All ]
```

- **Idle:** `variant="outline"`, icon `Zap`, label "Test All".
- **In progress:** spinner icon, label "Testing…", `disabled`.
- Calls `POST /admin/api/models/test-all`, dispatches results into context.
- On network error: `sonner` toast with error message; `testing` reset to `false`.

### Column — `models-columns.tsx`

Insert a `status` column after the existing `key_status` column:

| Condition | Display |
|---|---|
| No result yet | `—` muted dash |
| Testing in progress | `⋯` animated dots (muted) |
| `ok: true` | `✓ 342ms` green text |
| `ok: false` | `✗ ERR` red badge, full error in `title` tooltip |

- Error abbreviation: take up to 12 characters of `error`, append `…` if truncated. Full text exposed via HTML `title` attribute for hover tooltip.
- The column reads `testResults` and `testing` from context.

---

## Data Flow

```
User clicks "Test All"
  → testing = true, testResults = new Map()
  → POST /admin/api/models/test-all
      → backend: join_all probe per model
      → returns [{id, ok, latency_ms|error}]
  → populate testResults Map
  → testing = false
  → Status column re-renders per row
```

---

## Files Changed

| File | Change |
|---|---|
| `src/admin/api.rs` | Add `test_all_models` handler + route `POST /admin/api/models/test-all` |
| `web/src/features/models/components/models-provider.tsx` | Add `testing`, `testResults`, `testAll` to context |
| `web/src/features/models/components/models-primary-buttons.tsx` | Add "Test All" button |
| `web/src/features/models/components/models-columns.tsx` | Add `status` column |
| `web/src/lib/api.ts` | Add `testAllModels()` helper (if not inlined) |

---

## Constraints

- Results are **in-memory only** — no DB reads or writes.
- Error strings are sanitized (truncated, no raw secrets) before leaving the backend.
- The endpoint is admin-token-gated; no inference-key access.
- No new dependencies.

---

## Addendum — v0.1.3 (auto-correction + token probing)

The shipped feature went beyond the original "in-memory only" scope above. This addendum
documents the actual behavior; where it conflicts with the sections above, the addendum wins.

### New endpoint

- `POST /admin/api/models/:id/test` — probe a single model (in addition to `test-all`).
  The edit dialog fires this automatically after a successful save, so a corrected
  `base_url`/`connector` is detected without a manual click.

### Auto-correction is persisted (DB writes)

`probe_one` no longer just measures latency. It:

1. **Finds a working `(connector, base_url)`** by trying the configured combo first, then
   variants: `/v1` suffix toggled, and `http`→`https` upgrade. Reachability probes use a tiny
   `max_tokens` (`PING_MAX_TOKENS = 8`) so an oversized value never makes a reachable endpoint
   look unreachable. A candidate is only accepted if the 200 response **parses into a
   recognizable completion** (has assistant text or a usage block) — a bare/health-page 200 is
   rejected so a wrong connector is never promoted.
2. **Probes the max output-token cap**: sends `PROBE_MAX_TOKENS = 1_000_000`; if rejected, parses
   the real upper bound from the error message (e.g. `[1, 393216]` → 393216), excluding the
   echoed request value, and re-probes to confirm before trusting it.
3. **Persists** any changed `connector` / `base_url` / `extra.default_max_tokens` via
   `model_upsert`. The frontend refetches `['models']` after a successful probe so the table and
   edit dialog reflect the corrected values.

The probe HTTP client (and the production egress client) use
`reqwest::redirect::Policy::none()` so an `http`→`https` redirect surfaces as a failure and the
`https` candidate is tried/persisted instead of being silently followed (which would also leak
the Anthropic `x-api-key` to the redirect target).

### Input vs output token limits (`extra`)

Two distinct keys are stored in the `extra` JSON column:

- `default_max_tokens` — auto-detected **output** cap (step 2). Read-only in the UI. Used as the
  request default when a client omits `max_tokens` (`req.max_tokens.or(ctx.default_max_tokens)`),
  and as Anthropic's required `max_tokens`. **Note:** this means a probed model's
  unspecified-`max_tokens` requests now default to the detected cap rather than the prior
  hard-coded 1024 (Anthropic) / provider default (OpenAI).
- `max_input_tokens` — user-editable **context window**, default 1,000,000. Informational only:
  it is **not** read by the backend or enforced (no tokenizer), so it does not gate requests.

### Backward compatibility

Both `extra` keys are optional. Rows without them behave as before: no output default is applied
and the input field shows its 1M default. No schema migration is required (the `extra` column
already exists).

### Security hardening (v0.1.3)

- `base_url` is validated on write: must be `http`/`https`, and link-local addresses
  (`169.254.0.0/16` / `fe80::/10`, the cloud-metadata range) are rejected to prevent SSRF.
  Loopback/LAN targets remain allowed (local model servers are a primary use case).
  Override with `LOCALFUSION_ALLOW_LINK_LOCAL=1`.
- Non-loopback server binds are refused unless `--allow-remote` is passed (traffic is plaintext
  HTTP; front with a TLS proxy when exposing remotely).
- Admin-token hash comparison is constant-time.
