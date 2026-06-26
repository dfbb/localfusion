# P5-T07 rust-embed 嵌入 + 构建串联

**阶段:** 5 前端 · **前置:** P5-T01..T06, P4-T07 · 见全局约束: `00-index.md`

**Goal:** 把 `web/dist` 经 rust-embed 编译期嵌入二进制，管理 server 从内存 serve；dist 缺失回退占位页（保证 Rust 核心可独立编译）；文档化「先 pnpm build 再 cargo build」（设计 §13.3）。

**Files:** Modify: `Cargo.toml`（加 rust-embed）、`src/admin/static_assets.rs`、`src/admin/mod.rs`（SPA fallback 路由）；Create: `README.md`（构建说明）

**Produces:** 单一二进制内嵌前端；访问管理端口根路径返回 SPA。

- [ ] **Step 1: 加依赖**

`Cargo.toml` `[dependencies]` 增加：`rust-embed = { version = "8", features = ["mime-guess"] }`。

- [ ] **Step 2: 实现 static_assets.rs（嵌入 + SPA fallback）**

```rust
use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

// dist 缺失时该目录可能为空——编译仍通过（RustEmbed 允许空目录）。
#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;

const PLACEHOLDER: &str = "<h1>LocalFusion</h1><p>前端未构建（web/dist 为空）。请用 /admin/api 管理，或运行 pnpm build 后重新 cargo build。</p>";

/// 静态资源 + SPA fallback：命中文件返回文件；否则返回 index.html（前端路由）；index 也缺失则占位页。
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response();
    }
    // SPA fallback → index.html
    if let Some(index) = Assets::get("index.html") {
        return ([(header::CONTENT_TYPE, "text/html")], index.data).into_response();
    }
    (StatusCode::OK, Html(PLACEHOLDER)).into_response()
}
```

> `mime_guess` 由 rust-embed 的 `mime-guess` feature 间接提供；若需直接用，加 `mime_guess = "2"`。

- [ ] **Step 3: admin/mod.rs 挂 SPA fallback**

在 `router()` 末尾加（放在 `/admin/api/*` 之后，作为兜底）：

```rust
use axum::routing::get;
// ...
    .fallback(get(static_assets::serve))
```

确保 `/admin/api/*` 路由优先匹配；非 API 路径走 SPA。删除 P4-T07 里临时的 `serve_index`（被 `serve` 取代）。

- [ ] **Step 4: 确保 web/dist 存在占位（首次编译）**

若尚未 `pnpm build`，创建空 `web/dist/.gitkeep` 让 `#[folder = "web/dist/"]` 路径存在（RustEmbed 对空目录回退占位页）。

- [ ] **Step 5: 写 README.md 构建说明**

```markdown
# LocalFusion 构建
1. 前端：`cd web && pnpm install && pnpm build`（产物到 web/dist）
2. 后端：`cargo build --release`（rust-embed 嵌入 web/dist）
3. 运行：`./target/release/localfusion --db ./localfusion.db`
   首次启动控制台打印 admin token（仅一次），用它登录管理端口 127.0.0.1:8788。
CI：两步串联（先 pnpm build，再 cargo build）。dist 缺失时后端仍可编译（前端显示占位页）。
```

- [ ] **Step 6: 全量验证 + 提交**

```bash
cd web && pnpm build && cd ..
cargo build --release && cargo test && cargo clippy --all-targets
git add Cargo.toml src/admin/ web/dist/.gitkeep README.md
git commit -m "feat: rust-embed 嵌入前端 + SPA fallback + 构建串联文档"
```

> **阶段 5 / 全项目完成**：单一可执行文件含完整后端 + 内嵌管理前端。手动验收：
> ```bash
> cd web && pnpm build && cd .. && cargo run --release -- --db /tmp/lf.db
> # 浏览器开 http://127.0.0.1:8788，用控制台打印的 admin token 登录，配置模型与虚拟模型
> # OpenAI SDK base_url 指向 http://127.0.0.1:8787/v1，model 填虚拟模型名，端到端验证
> ```
