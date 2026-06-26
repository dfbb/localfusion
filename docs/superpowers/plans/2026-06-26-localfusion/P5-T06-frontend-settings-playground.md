# P5-T06 设置/日志 + 调试台(playground)

**阶段:** 5 前端 · **前置:** P5-T01, P4-T09 · 见全局约束: `00-index.md`

**Goal:** `features/settings/`（日志配置）+ `features/playground/`（对虚拟模型发测试请求看编排细节）。前者对应 `/admin/api/settings/logging`；后者需新增后端 `POST /admin/api/playground`（设计 §13.2.5 / §13.2.6）。

**Files:**
- 后端: Modify `src/admin/api.rs`（加 playground 路由）、`src/admin/mod.rs`（merge）
- 前端: `web/src/features/settings/{index,components/logging-form}.tsx`、`web/src/features/playground/{index,components/{trace-view,calls-table}}.tsx`、`web/src/routes/_authenticated/{settings,playground}/index.tsx`

**Produces:** 设置/日志页 + 调试台页。

- [ ] **Step 1: 后端 playground 端点（admin/api.rs）**

```rust
pub fn playground_routes() -> Router<AdminState> {
    Router::new().route("/admin/api/playground", post(playground))
}

/// body: { virtual_name, prompt } → 跑一次该虚拟模型(want_stream=false, trace=Some)，返回编排细节。
async fn playground(State(s): State<AdminState>, h: HeaderMap, Json(body): Json<Value>) -> Response {
    if let Err(r) = require_admin(&s, &h).await { return r; }
    let vn = match body.get("virtual_name").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(), None => return err_response(crate::error::FusionError::InvalidRequest("virtual_name required".into())) };
    let prompt = body.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let req = crate::unified::UnifiedRequest {
        items: vec![crate::unified::Item::Message { role: crate::unified::Role::User,
            content: vec![crate::unified::ContentBlock::Text(prompt)] }],
        tools: vec![], max_tokens: Some(1024), temperature: None, stream: false, raw_extra: Value::Null };
    let vm = match s.db.vmodel_get(&vn).await { Ok(Some(v)) => v, Ok(None) =>
        return err_response(crate::error::FusionError::InvalidRequest("unknown virtual model".into())),
        Err(e) => return err_response(e) };
    let router = crate::router::Router::new(s.db.clone(), s.enc_key);
    let recorder = crate::unified::CallRecorder::default();
    let trace = crate::unified::StrategyTrace::default();
    let result = router.dispatch(&vn, req, false, &recorder, Some(&trace)).await;
    let calls = recorder.drain();
    match result {
        Ok(crate::strategy::StrategyOutput::Full(resp)) => {
            let final_text = resp.items.iter().find_map(|i| match i {
                crate::unified::Item::Message { content, .. } => Some(content.iter().filter_map(|c| match c {
                    crate::unified::ContentBlock::Text(t) => Some(t.clone()), _ => None }).collect::<String>()),
                _ => None }).unwrap_or_default();
            Json(serde_json::json!({"final": final_text, "strategy": vm.strategy,
                "status": trace.snapshot().get("status").cloned().unwrap_or(Value::Null),
                "calls": calls, "detail": trace.snapshot()})).into_response()
        }
        Ok(_) => err_response(crate::error::FusionError::Internal("unexpected stream in playground".into())),
        Err(e) => Json(serde_json::json!({"error": e.to_string(), "calls": calls, "detail": trace.snapshot()})).into_response(),
    }
}
```
在 `admin/mod.rs` router 加 `.merge(api::playground_routes())`。补集成测试（`tests/admin_api.rs` 加 playground 用例，用 mock 模型不便，可仅断言 400/缺字段路径）。

- [ ] **Step 2: 后端 build + 提交**

```bash
cargo test && cargo clippy --all-targets
git add src/admin/
git commit -m "feat: playground 管理端点(trace 编排细节)"
```

- [ ] **Step 3: 前端 settings/logging-form.tsx**

```ts
const { data } = useQuery({ queryKey:['logging'], queryFn: ()=>api.get('/settings/logging').then(r=>r.data) })
const save = useMutation({ mutationFn: (v)=>api.put('/settings/logging', v),
  onSuccess: ()=>{ qc.invalidateQueries({queryKey:['logging']}); toast.success('已保存(文件/控制台改动需重启)') } })
```
表单：log_level Select（debug/info/error）、log_file 文本框、log_to_stdout Switch。左侧 `sidebar-nav` 子导航（日志 / 服务器只读）。

- [ ] **Step 4: 前端 playground**

```ts
const run = useMutation({ mutationFn: (v)=>api.post('/playground', v).then(r=>r.data) })
```
表单：虚拟模型 Select（来自 vmodels query）+ 多行 prompt + 发送。结果按 `detail` 渲染（`trace-view.tsx`）：
- panel（synthesize/best-of-n）：左侧 `detail.member_answers` 卡片，右侧 `detail.judge.{input,output}`，顶部 `detail.status` badge。
- 单模型（failover/speed/cheapest）：`detail.attempts`（尝试链）/ `detail.candidates`（吞吐/成本对比）。
- multimodal：`detail.turns` 时间线。
底部 `calls-table.tsx` 展示本次 `calls`（model_id/role/tokens/cost/status/estimated/latency_secs）。

- [ ] **Step 5: 路由 + 验证 + 提交**

`routes/_authenticated/settings/index.tsx`、`routes/_authenticated/playground/index.tsx`。
```bash
cd web && pnpm build
cd .. && git add web/src/features/settings web/src/features/playground web/src/routes/_authenticated/settings web/src/routes/_authenticated/playground
git commit -m "feat(web): 设置/日志页 + 调试台(策略编排 trace 可视化)"
```
