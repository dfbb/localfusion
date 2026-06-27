# Model Connectivity Test — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Test All" button to the real-models page that probes every model for connectivity and latency, displaying results in a new Status column (in-memory only).

**Architecture:** New `POST /admin/api/models/test-all` backend endpoint runs all probes concurrently and returns `[{id, ok, latency_ms|error}]`. The frontend stores results in a `Map<string, TestResult>` inside `ModelsProvider` context; the Status column and Test All button read from that context.

**Tech Stack:** Rust (axum, futures, tokio), React 19, TypeScript, TanStack Table, shadcn/ui, axios.

## Global Constraints

- All code comments and doc strings in English.
- No new crate or npm dependencies.
- Results are in-memory only — no DB writes.
- The endpoint is admin-token-gated (`require_admin` middleware).
- Error strings truncated to 60 chars before leaving the backend (`upstream_error` helper already exists in `src/connector/mod.rs`).
- Follow existing patterns: axum handler structure matches other handlers in `src/admin/api.rs`; React context pattern matches `models-provider.tsx`.

---

### Task 1: Backend — `POST /admin/api/models/test-all` endpoint

**Files:**
- Modify: `src/probe.rs` — make `probe_request()` `pub(crate)`
- Modify: `src/admin/api.rs` — add handler + register route

**Interfaces:**
- Consumes: `crate::probe::probe_request()`, `crate::router::ModelResolver::resolve()`, `crate::connector::upstream_error()`, `AdminState { db, enc_key }`
- Produces: `POST /admin/api/models/test-all` → `200 OK` JSON array

- [ ] **Step 1: Make `probe_request` pub(crate)**

In `src/probe.rs`, change line 14:
```rust
fn probe_request() -> UnifiedRequest {
```
to:
```rust
pub(crate) fn probe_request() -> UnifiedRequest {
```

- [ ] **Step 2: Write failing test for the new endpoint**

Add to `tests/admin_api.rs`, following the exact pattern of existing tests there (uses `app()` helper + `tower::ServiceExt::oneshot`):

```rust
#[tokio::test]
async fn test_all_models_empty_db_returns_empty_array() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = app().await; // existing helper: in-memory DB, token="adm", enc_key=[0u8;32]

    let r = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/models/test-all")
                .header("Authorization", "Bearer adm")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(r.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 0); // no models in empty DB
}
```

- [ ] **Step 3: Run the test to verify it fails**

```bash
cd /path/to/localfusion
cargo test --test admin_api test_all_models 2>&1 | tail -20
```

Expected: compile error or test failure — handler does not exist yet.

- [ ] **Step 4: Implement the handler in `src/admin/api.rs`**

Add after the existing `delete_model` handler and before `pub fn vmodels_routes()`:

```rust
// ─── model connectivity test ───────────────────────────────────────────────

pub fn models_test_routes() -> Router<AdminState> {
    Router::new().route("/admin/api/models/test-all", post(test_all_models))
}

async fn test_all_models(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let models = match s.db.model_list().await {
        Ok(m) => m,
        Err(e) => return err_response(e),
    };

    let resolver = crate::router::ModelResolver::new(s.db.clone(), s.enc_key);

    let tasks: Vec<_> = models
        .into_iter()
        .map(|m| {
            let resolver = resolver.clone();
            let id = m.id.clone();
            async move {
                let start = std::time::Instant::now();
                match resolver.resolve(&id).await {
                    Err(_) => serde_json::json!({
                        "id": id,
                        "ok": false,
                        "error": "key unavailable"
                    }),
                    Ok(member) => {
                        let req = crate::probe::probe_request();
                        match member.connector.complete(&req, &member.egress).await {
                            Ok(_) => {
                                let ms = start.elapsed().as_millis() as u64;
                                serde_json::json!({ "id": id, "ok": true, "latency_ms": ms })
                            }
                            Err(e) => {
                                // Truncate error to 60 chars for the response
                                let msg = e.to_string();
                                let short: String = msg.chars().take(60).collect();
                                let short = if msg.chars().count() > 60 {
                                    format!("{short}…")
                                } else {
                                    short
                                };
                                serde_json::json!({ "id": id, "ok": false, "error": short })
                            }
                        }
                    }
                }
            }
        })
        .collect();

    let results = futures::future::join_all(tasks).await;
    Json(results).into_response()
}
```

Add the necessary imports at the top of `src/admin/api.rs` if not already present:
```rust
use axum::routing::post; // already imported
// futures is already in Cargo.toml
```

- [ ] **Step 5: Register the route in `src/admin/mod.rs`**

In `src/admin/mod.rs`, inside `pub fn router(state: AdminState) -> Router`, add `.merge(api::models_test_routes())`:

```rust
pub fn router(state: AdminState) -> Router {
    let cors = /* existing cors setup */;
    Router::new()
        .route("/admin/api/health", get(health))
        .merge(api::models_routes())
        .merge(api::models_test_routes())   // ← add this line
        .merge(api::vmodels_routes())
        /* rest unchanged */
}
```

- [ ] **Step 6: Check that `ModelResolver` is `Clone`**

```bash
grep -n "derive.*Clone\|impl Clone" src/router.rs
```

If `ModelResolver` does not derive or implement `Clone`, add `#[derive(Clone)]` to its struct definition in `src/router.rs`.

- [ ] **Step 7: Run the test to verify it passes**

```bash
cargo test --test admin_api test_all_models 2>&1 | tail -10
```

Expected:
```
test test_all_models_empty_returns_empty_array ... ok
```

- [ ] **Step 8: Run full test suite**

```bash
cargo test 2>&1 | grep -E "test result|FAILED"
```

Expected: all pass, 0 failed.

- [ ] **Step 9: Commit**

```bash
git add src/probe.rs src/admin/api.rs src/admin/mod.rs src/router.rs tests/admin_api.rs
git commit -m "feat: POST /admin/api/models/test-all — concurrent model connectivity probe"
```

---

### Task 2: Frontend — provider state, button, status column

**Files:**
- Modify: `web/src/features/models/components/models-provider.tsx`
- Modify: `web/src/features/models/components/models-primary-buttons.tsx`
- Modify: `web/src/features/models/components/models-columns.tsx`

**Interfaces:**
- Consumes: `POST /admin/api/models/test-all` → `[{id, ok, latency_ms?, error?}]`
- Produces: `useModels()` now also returns `{ testing, testResults, runTestAll }`

- [ ] **Step 1: Update `models-provider.tsx`**

Replace the entire file content with:

```tsx
import React, { useState } from 'react'
import { toast } from 'sonner'
import { api } from '@/lib/api'
import { type ModelRow } from '../data/schema'

type ModelsDialogType = 'add' | 'edit' | 'delete'

export type TestResult =
  | { ok: true; latency_ms: number }
  | { ok: false; error: string }

type ModelsContextType = {
  open: ModelsDialogType | null
  setOpen: (str: ModelsDialogType | null) => void
  currentRow: ModelRow | null
  setCurrentRow: React.Dispatch<React.SetStateAction<ModelRow | null>>
  testing: boolean
  testResults: Map<string, TestResult>
  runTestAll: () => Promise<void>
}

const ModelsContext = React.createContext<ModelsContextType | null>(null)

export function ModelsProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState<ModelsDialogType | null>(null)
  const [currentRow, setCurrentRow] = useState<ModelRow | null>(null)
  const [testing, setTesting] = useState(false)
  const [testResults, setTestResults] = useState<Map<string, TestResult>>(new Map())

  async function runTestAll() {
    setTesting(true)
    setTestResults(new Map())
    try {
      const resp = await api.post<Array<{ id: string; ok: boolean; latency_ms?: number; error?: string }>>(
        '/models/test-all'
      )
      const map = new Map<string, TestResult>()
      for (const item of resp.data) {
        map.set(item.id, item.ok
          ? { ok: true, latency_ms: item.latency_ms! }
          : { ok: false, error: item.error ?? 'unknown error' }
        )
      }
      setTestResults(map)
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Test failed'
      toast.error(msg)
    } finally {
      setTesting(false)
    }
  }

  return (
    <ModelsContext value={{ open, setOpen, currentRow, setCurrentRow, testing, testResults, runTestAll }}>
      {children}
    </ModelsContext>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function useModels() {
  const ctx = React.useContext(ModelsContext)
  if (!ctx) throw new Error('useModels must be used within <ModelsProvider>')
  return ctx
}
```

- [ ] **Step 2: Update `models-primary-buttons.tsx`**

Replace the entire file content with:

```tsx
import { Loader2, Plus, Zap } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useModels } from './models-provider'

export function ModelsPrimaryButtons() {
  const { setOpen, testing, runTestAll } = useModels()
  return (
    <div className="flex items-center gap-2">
      <Button onClick={() => setOpen('add')}>
        <Plus className="mr-2 h-4 w-4" />
        New Model
      </Button>
      <Button variant="outline" onClick={runTestAll} disabled={testing}>
        {testing ? (
          <>
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            Testing…
          </>
        ) : (
          <>
            <Zap className="mr-2 h-4 w-4" />
            Test All
          </>
        )}
      </Button>
    </div>
  )
}
```

- [ ] **Step 3: Add Status column to `models-columns.tsx`**

Add this new column definition after the `key_status` column entry (before the `actions` column), and add the necessary import for `useModels`:

The file already imports `useModels`. Add a `StatusCell` component and insert the column:

```tsx
// Add this component inside models-columns.tsx, before modelsColumns definition:
function StatusCell({ modelId }: { modelId: string }) {
  const { testing, testResults } = useModels()
  const result = testResults.get(modelId)

  if (testing && !result) {
    return <span className="text-muted-foreground text-sm">⋯</span>
  }
  if (!result) {
    return <span className="text-muted-foreground text-sm">—</span>
  }
  if (result.ok) {
    return (
      <span className="text-green-600 dark:text-green-400 text-sm font-mono">
        ✓ {result.latency_ms}ms
      </span>
    )
  }
  const short = result.error.length > 12
    ? result.error.slice(0, 12) + '…'
    : result.error
  return (
    <span
      className="text-red-600 dark:text-red-400 text-sm font-mono cursor-default"
      title={result.error}
    >
      ✗ {short}
    </span>
  )
}
```

Then add this column to `modelsColumns` after the `key_status` entry:

```tsx
  {
    id: 'status',
    header: 'Status',
    cell: ({ row }) => <StatusCell modelId={row.original.id} />,
  },
```

Full final `modelsColumns` array for reference (all columns in order):
1. `id`
2. `connector`
3. `model`
4. `base_url`
5. `key_status`
6. `status` ← new
7. `actions`

- [ ] **Step 4: Build the frontend to verify no TypeScript errors**

```bash
cd web && pnpm build 2>&1 | tail -15
```

Expected:
```
✓ built in ...
```

No TypeScript errors. If there are type errors, fix them before proceeding.

- [ ] **Step 5: Run backend tests to confirm nothing broken**

```bash
cd .. && cargo test 2>&1 | grep -E "test result|FAILED"
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add web/src/features/models/components/
git commit -m "feat(web): Test All button + Status column for real-models page"
```

---

### Task 3: Build and smoke test

- [ ] **Step 1: Full release build**

```bash
cd web && pnpm build && cd .. && cargo build --release 2>&1 | tail -5
```

Expected:
```
Finished `release` profile [optimized] target(s) in ...
```

- [ ] **Step 2: Smoke test manually**

```bash
mkdir -p target/tmp
./target/release/localfusion --db target/tmp/localfusion.db
```

1. Open `http://127.0.0.1:8788/` and log in.
2. Navigate to Real Models.
3. Verify "Test All" button appears to the right of "New Model".
4. Click "Test All":
   - Button shows spinner + "Testing…" while in flight.
   - Status column shows `⋯` for all rows.
   - After response: shows `✓ NNNms` (green) or `✗ ERR…` (red) per row.
5. Hover over a red `✗` cell — full error message appears in tooltip.
6. Click "Test All" again — results reset and re-run.

- [ ] **Step 3: Commit and push**

```bash
git add -A
git commit -m "chore: release build verified for model connectivity test feature" 2>/dev/null || true
git push
```
