# LocalFusion 实现计划 — 索引

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans。**每个 task 是本目录下的一个独立文件**，按下方顺序逐个执行。每个 task 文件内含完整代码与 TDD 步骤（checkbox），且重复声明其依赖与接口，可独立阅读。

**Goal:** 用 Rust 实现 LocalFusion——本地独立运行的多供应商 LLM 混合调度代理（单一可执行文件，三协议入口 + 6 策略 + 管理 Web）。设计见 `../2026-06-26-localfusion-design.md`。

**Architecture:** 三层正交（入口协议 / 调度策略 / 出口供应商）由统一中间表示解耦；所有配置/状态存 SQLite；前端编译产物经 rust-embed 嵌入同一二进制。

**Tech Stack:** Rust 2021、tokio、axum、sqlx(SQLite)、reqwest、serde、chacha20poly1305、tracing；前端 React 19 + Vite + TanStack + shadcn/ui。

## Global Constraints（所有 task 隐含适用）

- Rust edition 2021；对话与文档用中文。
- SQLite 库固定 **sqlx**（`features=["runtime-tokio","sqlite","macros"]`）；**每连接执行** `PRAGMA foreign_keys=ON; journal_mode=WAL; busy_timeout=5000`（设计 §4）。
- 加密固定 **ChaCha20-Poly1305**；`key=HKDF-SHA256(ikm=machine-id, salt=enc_salt)`；密文 `base64(nonce[12]||ct||tag)`；仅 `models.api_key_enc` 加密；ingress key/admin token 存 **SHA-256 哈希**（设计 §5）。
- 统计单一权威：`CallRecorder.drain()` + 流式 `Done.call`/`Error.call`，**不读** `UnifiedResponse.calls`（设计 §6.1/§6.3/§8）。
- `want_stream` 分支：panel 类 + multimodal 恒 `Full`；单模型类 `true`→`Stream`、`false`→`Full`（设计 §6.3）。
- multimodal 固定 buffered（设计 §7.6）。
- 安全：错误不泄密；日志绝不记任何明文密钥（设计 §5.3）。admin token 首启仅 `println!` 直接输出，不经 tracing（设计 §3/§9）。
- 默认 bind：推理 `127.0.0.1:8787`、管理 `127.0.0.1:8788`。
- 每个 task 结束：`cargo build` + `cargo test` + `cargo clippy` 通过并提交。

## 执行顺序

### 阶段 1：基础层（脚手架 + 加密 + 类型 + DB）
- [ ] P1-T01-scaffold.md — 工程脚手架
- [ ] P1-T02-error.md — FusionError
- [ ] P1-T03-crypto.md — 加密/哈希/派生
- [ ] P1-T04-unified.md — 统一类型 + CallRecorder + StrategyTrace + ConnError
- [ ] P1-T05-db-pool.md — DB 连接池/迁移/PRAGMA
- [ ] P1-T06-db-settings.md — settings 读写
- [ ] P1-T07-db-models.md — models CRUD
- [ ] P1-T08-db-virtual-models.md — virtual_models/members + 引用检查
- [ ] P1-T09-db-keys.md — ingress_keys/ACL + 鉴权
- [ ] P1-T10-db-prices.md — prices 读
- [ ] P1-T11-db-latency.md — latency 写入 + speed 查询
- [ ] P1-T12-db-usage.md — usage_hourly + request_log

### 阶段 2：Connector 层
- [ ] P2-T01-connector-core.md — Connector trait + EgressCtx + url/header/key
- [ ] P2-T02-sse-engine.md — SSE 出口引擎
- [ ] P2-T03-chat-connector.md — ChatConnector
- [ ] P2-T04-anthropic-connector.md — AnthropicConnector
- [ ] P2-T05-responses-connector.md — ResponsesConnector

### 阶段 3：策略 + Router
- [ ] P3-T01-strategy-core.md — Strategy trait + 注册表 + call_member + schema
- [ ] P3-T02-failover.md — failover（含 mock 测试设施）
- [ ] P3-T03-speed.md — speed
- [ ] P3-T04-cheapest.md — cheapest
- [ ] P3-T05-synthesize.md — synthesize
- [ ] P3-T06-best-of-n.md — best-of-n
- [ ] P3-T07-multimodal.md — multimodal
- [ ] P3-T08-router.md — ModelResolver + Router

### 阶段 4：入口 / 鉴权 / 管理 API / 装配（完成 Spec A）
- [ ] P4-T01-logging.md — 日志系统
- [ ] P4-T02-ingress-chat.md — OpenAI Chat 入口
- [ ] P4-T03-ingress-responses.md — OpenAI Responses 入口
- [ ] P4-T04-ingress-anthropic.md — Anthropic 入口
- [ ] P4-T05-auth.md — 鉴权
- [ ] P4-T06-pipeline.md — 出口统计落库
- [ ] P4-T07-admin-api.md — 管理 REST API
- [ ] P4-T08-probe.md — speed 探测后台任务
- [ ] P4-T09-main.md — 冷启动引导 + 推理 handler + main 装配

### 阶段 5：前端（Spec B，消费管理 API）
- [ ] P5-T01-frontend-scaffold.md — Vite/TanStack/shadcn 脚手架 + 鉴权 + 布局
- [ ] P5-T02-frontend-models.md — 真实模型页
- [ ] P5-T03-frontend-virtual-models.md — 虚拟模型页
- [ ] P5-T04-frontend-keys.md — 密钥/ACL 页
- [ ] P5-T05-frontend-dashboard.md — 监控面板
- [ ] P5-T06-frontend-settings-playground.md — 设置/日志 + 调试台
- [ ] P5-T07-embed.md — rust-embed 嵌入 + 占位回退 + 构建串联
