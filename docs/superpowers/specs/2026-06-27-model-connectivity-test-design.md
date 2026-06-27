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
