# LocalFusion 设计文档

> 本地独立运行的多供应商 LLM 混合调度代理。单一 Rust 可执行文件，对外暴露
> OpenAI / OpenAI Responses / Anthropic 三套兼容端点，对内按「虚拟模型 → 策略 →
> 真实模型组」扇出到多家供应商，内置管理 Web 界面。

- 日期：2026-06-26
- 状态：设计已确认，待写实现计划
- 参考代码：`../3rd/llm-switch`（connector 转换逻辑，参考重写）、`../3rd/OpenPrism`（混合调度思想）

---

## 1. 目标与范围

LocalFusion 是一个本地运行的 Rust 二进制。它：

1. 对外暴露三套兼容 HTTP 端点（OpenAI Chat Completions、OpenAI Responses、Anthropic
   Messages），标准 SDK 零改动即可连接。
2. 对内把客户端请求里的「虚拟模型名」映射到一种**调度策略** + 一组**真实后端模型**，
   扇出调用多家供应商。
3. 提供 6 种调度策略，每种是一个插件：`failover` / `speed` / `cheapest` /
   `synthesize` / `best-of-n` / `multimodal`。
4. 所有配置与运行时状态存 SQLite（无 config.toml）。
5. 内置一个独立的管理 Web 服务（admin token 鉴权），用现代前端（React + Vite +
   shadcn）管理模型、策略、密钥、ACL，并查看监控。前端编译产物嵌入同一个二进制。

**最终产物自始至终是一个独立的 Rust 可执行文件。**

### 实现分期（同一开发周期，前端排最后）

- **Spec A（先做）**：Rust 代理核心——三入口、统一中间表示、Router、6 个策略插件、
  Connector、SQLite 读写层、推理路径鉴权、加密、日志系统、小时聚合统计、以及一套
  完整的管理 REST API。完成后用 curl 即可全量管理。
- **Spec B（后做）**：React 管理前端，消费 Spec A 的管理 REST API，编译产物用
  `rust-embed` 嵌入二进制。

---

## 2. 总体架构

请求自上而下流动，三层正交：**入口协议** / **调度策略** / **出口供应商** 互不知道
对方细节，由统一中间表示解耦。

```
              ┌────────────────────────────────────────────┐
   客户端 ───▶ │  Ingress 层 (axum)                          │
 OpenAI SDK    │   /v1/chat/completions   (OpenAI Chat)      │
 Anthropic SDK │   /v1/responses          (OpenAI Responses) │
 (Responses)   │   /v1/messages           (Anthropic)        │
              │   入口 key 校验 + ACL                        │
              └───────────────┬────────────────────────────┘
                              ▼
              统一中间表示 UnifiedRequest（协议无关，以 Responses item 为超集底座）
                              ▼
              ┌────────────────────────────────────────────┐
              │  Router: virtual_model 名 → 策略 + 真实模型组 │  ← 查 SQLite
              └───────────────┬────────────────────────────┘
                              ▼
              ┌────────────────────────────────────────────┐
              │  Strategy 插件 (trait Strategy)             │
              │   synthesize / best-of-n / failover /       │
              │   speed / cheapest / multimodal             │
              └───────────────┬────────────────────────────┘
                              ▼
              ┌────────────────────────────────────────────┐
              │  Connector 层 (trait Connector)             │  ← 参考 llm-switch 重写
              │   ChatConnector / AnthropicConnector /      │
              │   ResponsesConnector（出口转换 + SSE 翻译） │
              └───────────────┬────────────────────────────┘
                              ▼
                多家供应商（各自 base_url + 加密 key + 真实模型名）

      ┌──────────────────────────────────────────────────────────┐
      │  管理 Web 服务 (独立监听端口, admin token 鉴权)            │
      │   /admin/api/*  REST API  +  内嵌 React 前端 (rust-embed)  │
      └──────────────────────────────────────────────────────────┘

      后台任务: speed 探测 (tokio interval)、小时用量聚合 upsert
      共享状态后端: SQLite (配置 + 价格 + 延迟 + 密钥 + 用量统计)
```

### 核心解耦器：统一中间表示

- 入口把 Chat / Responses / Anthropic 请求都翻译成 `UnifiedRequest`。
- 策略和 Connector 只认 `UnifiedRequest` / `UnifiedResponse`。
- 出口再翻译回目标供应商格式。
- 入口协议、调度策略、出口供应商三者完全正交：一个 OpenAI 入口的请求完全可以经
  AnthropicConnector 打到 Claude 后端。

统一中间表示**以 OpenAI Responses 的 item 模型为超集底座**（表达力最强，能容纳
reasoning、内置工具 web_search/image_generation、多模态 item），Chat 与 Anthropic
入口作为它的子集投影。这同时天然支撑 multimodal 策略对内置工具的拦截。

---

## 3. 配置与冷启动

**所有配置存 SQLite，无 config.toml。** 唯一不进 DB 的是数据库文件本身的位置。

- 数据库路径：命令行 `--db <path>`，默认 `./localfusion.db`。
- 首次启动（空库）引导：
  1. 检测到空库 → 自动建表（迁移）。
  2. 生成随机 `enc_salt` 写入 settings。
  3. 生成一个随机 **admin token**，**经一次性 `println!` 直接写控制台**（只此一次，
     仅存哈希；**不经 tracing/日志系统**，避免落入日志文件，见 §5.3 / §9），用户拿它
     登录管理界面配置其余一切。
  4. 写入默认监听地址：推理入口 `127.0.0.1:8787`、管理端口 `127.0.0.1:8788`。
  5. 日志默认 level=info、输出 stdout、不写文件。
- 之后所有配置在管理界面修改；监听地址 / 日志文件路径类改动需重启生效（界面提示）。
- fail-safe：DB 不可读 / 解密失败 / 必需字段缺失 → 启动期明确报错退出，不带病运行。

---

## 4. SQLite Schema

数据库是配置 + 运行时状态中枢。

```sql
-- 服务器/全局配置 (kv)
CREATE TABLE settings (
  key   TEXT PRIMARY KEY,   -- inference_bind / admin_bind / admin_token_hash /
                            -- enc_salt / log_level / log_file / log_to_stdout
  value TEXT NOT NULL
);

-- 真实后端模型
CREATE TABLE models (
  id          TEXT PRIMARY KEY,   -- 'gpt-4o'
  connector   TEXT NOT NULL,      -- 'chat' | 'anthropic' | 'responses' (出口格式)
  base_url    TEXT NOT NULL,
  api_key_enc TEXT,               -- ChaCha20-Poly1305 密文 (nonce||ct||tag, base64)
  api_key_env TEXT,               -- 或指向环境变量 (二选一)
  model       TEXT NOT NULL,      -- 发给供应商的真实模型名
  anthropic_version TEXT,
  extra       TEXT                -- JSON, 连接器私有字段(如 default_max_tokens)
);

-- 虚拟模型 (对外暴露的模型名)
CREATE TABLE virtual_models (
  name     TEXT PRIMARY KEY,      -- 客户端请求的模型名
  strategy TEXT NOT NULL,         -- failover|speed|cheapest|synthesize|best-of-n|multimodal
  params   TEXT NOT NULL          -- JSON, 策略私有参数 (judge / 能力路由表 / 阈值...)
);

-- 虚拟模型 → 成员真实模型 (有序)
CREATE TABLE virtual_model_members (
  virtual_name TEXT NOT NULL REFERENCES virtual_models(name) ON DELETE CASCADE,
  model_id     TEXT NOT NULL REFERENCES models(id),
  position     INTEGER NOT NULL,  -- 顺序 (failover 优先级 / 展示)
  PRIMARY KEY (virtual_name, model_id)
);

-- 入口密钥 + ACL (存哈希, 不存明文)
CREATE TABLE ingress_keys (
  id         INTEGER PRIMARY KEY,
  key_hash   TEXT NOT NULL UNIQUE,   -- SHA-256
  label      TEXT,
  enabled    INTEGER NOT NULL DEFAULT 1,
  acl_all    INTEGER NOT NULL DEFAULT 0,  -- 1 = 允许全部虚拟模型 (wildcard, 取代 '*')
  created_at INTEGER NOT NULL
);
-- 具体白名单: 外键引用 virtual_models, 删除虚拟模型时级联清除对应 ACL 行,
-- 避免「删除后重建同名虚拟模型意外继承旧权限」。wildcard 不在此表 (用 acl_all)。
CREATE TABLE ingress_key_acl (
  key_id       INTEGER NOT NULL REFERENCES ingress_keys(id) ON DELETE CASCADE,
  virtual_name TEXT NOT NULL REFERENCES virtual_models(name) ON DELETE CASCADE,
  PRIMARY KEY (key_id, virtual_name)
);
```

> **ACL 语义**：鉴权时——`acl_all=1` 放行任意虚拟模型；否则要求请求的 virtual_name
> 出现在该 key 的 `ingress_key_acl` 行中。删除虚拟模型经外键级联自动清除其残留 ACL
> 行（重建同名虚拟模型不会继承旧授权，需重新授予）。前端「允许全部」开关写 `acl_all`，
> 「指定白名单」写具体行。**外键级联依赖连接初始化执行 `PRAGMA foreign_keys=ON`
> （见 §4 末「DB 层约束」）。**

```sql
-- (schema 续)

-- 价格 (第三方程序写入, localfusion 只读)
CREATE TABLE prices (
  model_id   TEXT PRIMARY KEY,       -- 对应 models.id
  price_in   REAL NOT NULL,          -- 每百万输入 token 单价
  price_out  REAL NOT NULL,          -- 每百万输出 token 单价
  updated_at INTEGER NOT NULL
);

-- 延迟样本 (speed 策略 + 探测任务写入)
CREATE TABLE latency_samples (
  id          INTEGER PRIMARY KEY,
  model_id    TEXT NOT NULL,
  tokens_out  INTEGER NOT NULL,
  output_secs REAL NOT NULL,         -- 产出 token 的耗时
  throughput  REAL NOT NULL,         -- tokens_out / output_secs (冗余存, 便于查询)
  is_probe    INTEGER NOT NULL DEFAULT 0,  -- 1=定期探测, 0=真实请求
  created_at  INTEGER NOT NULL
);
CREATE INDEX idx_latency_model_time ON latency_samples(model_id, created_at);

-- 逐条请求明细 (playground / 排错)
CREATE TABLE request_log (
  id           INTEGER PRIMARY KEY,
  virtual_name TEXT,
  strategy     TEXT,
  status       TEXT,                 -- 'ok'|'degraded'|'error'
  total_tokens INTEGER,
  cost         REAL,
  created_at   INTEGER NOT NULL
);

-- 按小时 × 维度 预聚合的用量累计
CREATE TABLE usage_hourly (
  hour_ts       INTEGER NOT NULL,    -- 整点 Unix 时间戳
  scope         TEXT NOT NULL,       -- 'real' | 'virtual' | 'total'
  name          TEXT NOT NULL,       -- 真实模型id / 虚拟模型名 / '' (total)
  requests      INTEGER NOT NULL DEFAULT 0,
  input_tokens  INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens  INTEGER NOT NULL DEFAULT 0,
  cost          REAL    NOT NULL DEFAULT 0,
  errors        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (hour_ts, scope, name)
);
CREATE INDEX idx_usage_hour ON usage_hourly(hour_ts);
```

要点：

- **speed 查询**：取每个候选最近 10 条样本的平均吞吐，选最高者。注意必须用子查询
  限定「最近 10 条」再聚合——直接 `AVG(...) ORDER BY ... LIMIT 10` 是错的（SQLite 先
  聚合成单行，LIMIT 对单行无意义）：
  ```sql
  SELECT AVG(throughput) FROM (
    SELECT throughput FROM latency_samples
    WHERE model_id = ?
    ORDER BY created_at DESC
    LIMIT 10
  );
  ```
- **cheapest 查询**：候选 join prices，按 `price_in×估算输入 + price_out×估算输出`
  选最低；缺价格者记 warning 排到最后。
- **价格/延迟解耦**：prices 由第三方写、localfusion 只读；latency 由 localfusion 自己
  写（真实请求 + 探测）。价格抓取程序 v1 不开发，测试用 seed 数据填充。
- **DB 层约束（外键必须显式开启）**：SQLite 默认 **不强制外键**。DB 层必须在**每个
  连接建立时**执行 `PRAGMA foreign_keys = ON`（连接池则配 `after_connect` 钩子对每条
  连接执行），否则 `ON DELETE CASCADE`（virtual_model_members、ingress_key_acl）与
  引用完整性（models 被引用检查）都不会生效。同时建议 `PRAGMA journal_mode = WAL`
  与 `PRAGMA busy_timeout` 以适配并发读写。迁移与该 PRAGMA 在启动建连时统一应用。

---

## 5. 安全与加密

### 5.1 provider api_key 对称加密

后端 provider 的 key 调用供应商时需明文，不能哈希，故对称加密存储：

- **算法**：ChaCha20-Poly1305（AEAD，带认证标签防篡改）。
- **密钥派生**：`key = HKDF-SHA256(ikm = machine-id, salt = enc_salt)`。
  - machine-id 经 `machine-uid`/`machineid` crate 取本机唯一标识。
  - `enc_salt` 首启随机生成，存 settings 表。
- **每条密文独立 nonce**：随机 12 字节 nonce，存储格式 `nonce || ciphertext || tag`
  base64。
- **绑定效果**：DB 拷到别的机器（machine-id 变）派生密钥不同，provider key 解不开。
- 仅 `models.api_key_enc` 用此加密。

### 5.2 入口与管理鉴权

- **ingress key**：存 SHA-256 哈希。客户端请求须带其一（`Authorization`/`x-api-key`），
  校验哈希；并检查 ACL（`acl_all=1` 放行任意虚拟模型，否则要求请求的 virtual_name 在
  该 key 的 `ingress_key_acl` 白名单中，见 §4 ACL 语义）。
  空 key 表（首启）时由界面尽快配置；ingress_keys 表为空可配置为拒绝所有推理请求。
- **admin token**：存哈希。管理 API 校验 `Authorization: Bearer <admin-token>`。
- 推理入口默认仅监听 127.0.0.1；管理服务独立端口，admin token 鉴权。

### 5.3 通用安全约束

- 错误信息**不回显**：provider key、解密细节、内部路径。供应商原始错误可截断脱敏转述。
- 日志**绝不记录**：provider key 明文、ingress key 明文、admin token、解密后密钥
  （即使 debug 档）。
- 管理 API 创建 key 后明文仅返回一次，之后不可见。

---

## 6. 代码契约层（核心抽象）

### 6.1 统一中间表示

```rust
/// 协议无关的请求。三个入口都翻译成它。
pub struct UnifiedRequest {
    pub items: Vec<Item>,             // 对话历史 (以 Responses item 为超集)
    pub tools: Vec<ToolDef>,          // 含内置工具标记 (web_search/image_generation...)
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: bool,
    pub raw_extra: serde_json::Value, // 透传未识别字段, 出口尽量保真
}

pub enum Item {
    Message { role: Role, content: Vec<ContentBlock> },
    Reasoning { content: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
    ToolResult { id: String, content: Vec<ContentBlock> },
}

pub enum ContentBlock {
    Text(String),
    Image { /* url 或 base64 */ },
}

/// 单次底层调用的用量明细。一个虚拟请求可能产生多条
/// （panel 策略 = 每个成员 + judge 各一条；单模型策略 = 一条）。
pub struct ModelUsage {
    pub model_id: String,        // 被调用的真实模型 id
    pub role: CallRole,          // Member | Judge | Tool（用量归因/展示）
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: f64,               // 据 prices 估算；缺价格记 0
    pub status: CallStatus,      // Ok | Failed
    /// true = token 数为本地估算(上游未返回 usage), false = 上游真实给出。
    /// 影响统计可信度展示; estimated 的 cost 也按估算 token 计算。
    pub estimated: bool,
    /// 本次调用墙钟耗时(秒)。speed 策略据此与 output_tokens 算 throughput;
    /// playground calls 也展示它。失败调用记录到失败为止的耗时。
    pub latency_secs: f64,
}

pub enum CallRole { Member, Judge, Tool }
pub enum CallStatus { Ok, Failed }

/// 统一响应。策略产出它, 出口翻译回目标协议。
pub struct UnifiedResponse {
    pub items: Vec<Item>,        // 通常是 assistant Message, 可能含 ToolCall
    /// 对外暴露的合计用量（virtual 维度统计与客户端 usage 字段用）。
    pub usage: Usage,
    /// 实际生效的真实模型（panel 类=judge, 单模型类=被选中者）。
    pub model_id: String,
    /// 本次请求所有底层调用的逐条用量明细。
    /// usage_hourly 的 scope='real' 维度按 calls 逐条累计，
    /// scope='virtual'/'total' 按 usage 合计累计（见 §8）。
    pub calls: Vec<ModelUsage>,
}
```

> **用量统计契约**：`calls` 是支撑 §8 三维度累计的关键。Connector 的
> `complete`/`stream` 每完成一次底层供应商调用，填一条 `ModelUsage`；策略把各成员
> 与 judge 的调用合并进 `calls`，并把合计填入 `usage`。failover/speed/cheapest 等
> 单模型策略产出单条 `calls`；synthesize/best-of-n 产出 N 个成员 + 1 个 judge；
> multimodal 产出主模型多轮 + 各工具回合，role 分别标 Member/Tool。失败的成员调用
> 也记一条 `status=Failed`（cost/token 据实，多为 0），供 errors 维度统计。

#### UnifiedStream 事件模型

`UnifiedStream` 是协议无关的流式抽象：一个产出 `UnifiedStreamEvent` 的异步流
（实现为 `tokio::sync::mpsc::Receiver<Result<UnifiedStreamEvent, ConnError>>` 包装，
对齐 llm-switch `sse.rs` 的 channel 模式）。Connector 把供应商 SSE 翻译成它；入口层
再把它翻译成目标协议的 SSE（Chat / Responses / Anthropic 各自的事件格式）。

```rust
pub struct UnifiedStream {
    pub rx: tokio::sync::mpsc::Receiver<Result<UnifiedStreamEvent, ConnError>>,
    pub upstream_request_id: Option<String>,
}

pub enum UnifiedStreamEvent {
    /// 流已建立、上游返回 2xx 且收到首个有效事件。failover 用它界定
    /// 「首事件边界」: 收到 Started 之前可故障转移, 之后不可 (§7.1)。
    Started { model_id: String },
    /// 文本增量。
    TextDelta { text: String },
    /// 推理增量 (Responses/Anthropic reasoning, 可选)。
    ReasoningDelta { text: String },
    /// 工具调用增量/完成 (multimodal 据此拦截; 含 id/name/args 累积)。
    ToolCall { id: String, name: String, args: serde_json::Value },
    /// 正常结束。携带本次调用的最终用量, 出口层据此写 usage_hourly (§8)。
    Done { usage: ModelUsage, finish_reason: Option<String> },
    /// 流中途错误 (已开始吐 token 后上游中断等)。
    Error { message: String },
}
```

事件契约：

- **顺序**：`Started` 必为首事件；随后任意多个 `*Delta` / `ToolCall`；以恰好一个
  `Done` **或** `Error` 终止流，终止后 channel 关闭。
- **usage 传递**：仅 `Done` 携带 `ModelUsage`（从供应商 SSE 的 usage 事件解析；若上游
  不给 usage，则按已累计产出 token 估算并标注 `estimated`）。出口层在转发流给客户端
  的同时缓存这条 usage，流关闭后写入 usage_hourly。`Error` 终止时也补记一条
  `status=Failed` 的用量（token/cost 据实，多为 0）。
- **错误语义**：建连/状态码失败发生在产出流**之前**，直接返回 `Err(ConnError)`（不进
  流）；`Error` 事件专指「已 `Started` 之后」的中途失败。failover 因此只能在收到
  `Started` 前切换（§7.1）。
- **speed 度量**：`Done.usage` 含 output_tokens；出口层结合从 `Started` 到 `Done` 的
  耗时算出 throughput，写 latency_samples（§7.2）。
- **伪流**：panel 类策略返回 `Full`，出口层把 `UnifiedResponse` 切成一串
  `UnifiedStreamEvent`（`Started` → 若干 `TextDelta` → `Done`）再按目标协议 SSE 输出，
  与真流共用同一套「Unified 事件 → 协议 SSE」翻译器。

### 6.2 Connector trait（出口适配器）

参考 llm-switch 的 chat/anthropic 转换逻辑**用标准类型重写**，不引入 codex 依赖
（llm-switch 的 connector 返回 `codex_api::ResponseStream` 等专有类型，无法直接复用）。

```rust
#[async_trait]
pub trait Connector: Send + Sync {
    async fn complete(&self, req: &UnifiedRequest, ctx: &EgressCtx)
        -> Result<UnifiedResponse, ConnError>;
    async fn stream(&self, req: &UnifiedRequest, ctx: &EgressCtx)
        -> Result<UnifiedStream, ConnError>;
}
// 实现: ChatConnector / AnthropicConnector / ResponsesConnector
// EgressCtx 由 Router 从 models 表组装: base_url / 解密后的 key / model 名 / auth 等
```

#### llm-switch 参考映射（具体文件 → 函数 → 我们的对应物）

下表逐项列出从 `../3rd/llm-switch` 参考哪个文件的哪个函数、参考其什么逻辑、以及在
LocalFusion 里重写成什么。**只参考转换算法，不复制代码**——llm-switch 的输入是
`codex_api::ResponsesApiRequest`、输出是 `codex_api::ResponseEvent/ResponseStream`，
我们替换为 `UnifiedRequest` / `UnifiedResponse` / `UnifiedStream`。

| llm-switch 文件:函数 | 参考的逻辑 | LocalFusion 重写为 |
|---|---|---|
| `src/connector/chat_req.rs:42 build_chat_request` | 把请求体翻译成 OpenAI Chat JSON 的主流程 | `connector/chat.rs` 出口请求构建（入参 `UnifiedRequest`） |
| `chat_req.rs:220 map_message` / `:241 map_agent_message` | message 角色与内容块 → Chat `messages` 项 | 同上，源类型换成 `Item::Message` |
| `chat_req.rs:263 map_function_call_output` / `:291 content_items_to_text` | 工具结果 → Chat `tool` 消息 | 源类型换成 `Item::ToolResult` |
| `chat_req.rs:317 map_tools` / `:354 map_tool_choice` | 工具定义与 tool_choice 翻译 | 入参换成 `UnifiedRequest.tools` |
| `chat_req.rs:388 reorder_tool_messages` | tool 消息排序以满足 OpenAI 约束 | 原样参考算法 |
| `chat_req.rs:447 apply_field_downgrade` | 不支持字段的降级处理 | 按需保留，作用于出口 JSON |
| `src/connector/chat_sse.rs:37 ChatSseState` + `:57 push_chunk` / `:141 finish` | Chat SSE 增量解析状态机（含 tool call 累积 `ToolAcc`） | `connector/chat.rs` 的 SSE→Unified 事件翻译器 |
| `chat_sse.rs:212 map_usage` | 从 SSE 解析 token usage | 填充 `ModelUsage`（支撑 §8 统计） |
| `chat_sse.rs:201 map_end_turn` | finish_reason → 结束语义 | 同语义映射 |
| `src/connector/anthropic_req.rs:56 build_anthropic_request` | 翻译成 Anthropic Messages JSON | `connector/anthropic.rs` 出口请求构建 |
| `src/connector/anthropic_sse.rs:42 AnthropicSseState` + `:68 push_event` / `:174 finish` | Anthropic SSE 事件机（`content_block_*` 累积 `BlockAcc`） | `connector/anthropic.rs` SSE→Unified 翻译器 |
| `src/sse.rs:run_egress` | **SSE 出口引擎**：同步发 POST + 状态码校验后才 spawn；按字节边界 `\n\n` 切帧、仅在完整帧处做 UTF-8 解码（避免多字节字符截断）；`data:`/`[DONE]` 解析 | `connector/sse.rs` 的通用 SSE 引擎，产出 `UnifiedStream`（**保留这套字节级 UTF-8 安全切帧逻辑**） |
| `src/http.rs:egress_url` / `default_path` | base_url + path 拼接、按 connector 选默认 path | `connector/mod.rs` 的 URL 组装 |
| `src/http.rs:build_headers` | Bearer→`Authorization`、XApiKey→`x-api-key`+`anthropic-version`、注入 `Content-Type` | `connector/mod.rs` 的 header 构建 |
| `src/http.rs:resolve_key` | key_env → 环境变量 / 否则 inline | 改为：先解密 `api_key_enc`（§5.1），否则读 `api_key_env` |

新增（llm-switch 无对应，需自研）：**ResponsesConnector**（出口为 OpenAI Responses
协议）。llm-switch 把 Responses 视为 codex 原生路径不做转换，我们因要支持 Responses
出口与统一表示互转，需自行实现 `build_responses_request` 与 `response.*` SSE 事件机。

> 取舍说明：llm-switch 的中间表示恰好就是 Responses（`ResponsesApiRequest`），所以它
> 的 chat/anthropic connector 本质是「Responses ↔ Chat/Anthropic」双向转换。我们的
> `UnifiedRequest` 也以 Responses item 为底座（§2），因此转换算法可高度复用，差异仅在
> 把 codex 专有结构体字段换成我们的 `Item`/`ContentBlock`。

### 6.3 Strategy trait（编排器，每策略一插件）

```rust
#[async_trait]
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError>;
}

pub struct StrategyCtx<'a> {
    pub req: UnifiedRequest,
    pub members: Vec<MemberHandle<'a>>,  // 有序的成员句柄, 每个含 Connector + EgressCtx
    /// 模型解析器: 把任意真实模型 id 解析成可调用句柄。
    /// 供 judge / multimodal 能力路由使用——它们引用的 model id 不一定在 members 里。
    pub resolver: &'a ModelResolver<'a>,
    pub params: serde_json::Value,       // 该虚拟模型的策略私有参数
    pub db: &'a Db,                      // speed/cheapest 查延迟/价格表
    pub want_stream: bool,
    /// 调用记录器: 每次非流式底层调用(成功/失败)都推一条 ModelUsage。
    /// 编排层在策略返回后(Ok/Err 均) drain 写统计。见下文「调用记录器」。
    pub recorder: &'a CallRecorder,
    /// 调试 trace: 仅 playground 请求时为 Some(正常推理入口为 None)。
    /// 策略把成员答案/judge 输入输出/候选对比/尝试链/时间线写入它, 供 §13.2.6 还原。
    pub trace: Option<&'a StrategyTrace>,
}

/// 由 Router 构造, 持有 models 表 + 解密能力 + 共享 http client + 连接器工厂。
/// `resolve(id)` 查 models 表 → 解密 key → 组装 EgressCtx → 配 Connector, 返回句柄。
/// 未知 id 返回 Err(FusionError::InvalidRequest)。
pub struct ModelResolver<'a> { /* db, crypto, http, ... */ }
impl<'a> ModelResolver<'a> {
    pub fn resolve(&self, model_id: &str) -> Result<MemberHandle<'a>, FusionError>;
}

/// 成员/被解析模型句柄: 一个具体真实模型的可调用单元。
pub struct MemberHandle<'a> {
    pub model_id: String,
    pub connector: &'a dyn Connector,
    pub egress: EgressCtx,
}

pub enum StrategyOutput {
    Stream(UnifiedStream),    // 真流式: 某单模型原生流直接透传
    Full(UnifiedResponse),    // 完整响应: 出口层据 want_stream 决定是否切伪流 SSE
}
```

> **调用记录器（CallRecorder）——统计的唯一权威来源**：策略一次执行可能发起多次底层
> 调用，其中部分成功、部分失败（failover 的前置失败尝试、panel 某成员失败、judge 失败、
> 流式中途 `Error` 等）。这些"已发生但最终未成功返回"的调用同样要进 §8 统计，但
> `StrategyOutput` 的 `Stream`/`Full` 只能携带成功路径的数据，`Err(FusionError)` 更是
> 什么都带不回。为此 `StrategyCtx` 持有一个 `recorder: &CallRecorder`（内部可变累加器）：
>
> ```rust
> pub struct CallRecorder { /* 内部 Mutex<Vec<ModelUsage>> */ }
> impl CallRecorder {
>     pub fn record(&self, usage: ModelUsage);     // 每次非流式底层调用完成即推一条(成功或失败)
>     pub fn drain(&self) -> Vec<ModelUsage>;       // 编排层在策略返回后读取
> }
> ```
>
> 契约：
> - **每次非流式底层调用**（`MemberHandle` 的 `complete`）完成时——无论成功还是失败——
>   策略（或 MemberHandle 的包装）向 `recorder.record(..)` 推一条 `ModelUsage`
>   （失败的 `status=Failed`，token/cost 据实，多为 0）。
> - **每次流式尝试在 `Started` 之前失败**（`MemberHandle` 的 `stream()` 直接返回
>   `Err`，即建连/状态码/首事件失败）也必须 `recorder.record(..)` 推一条
>   `status=Failed` 的 `ModelUsage`。这覆盖**流式 failover 的前置失败尝试**——它跳过
>   失败成员去试下一个，每个失败成员都留一条记录，不漏统计。
> - **编排层**（Router 之上、出口层）在策略返回后，**无论 `Ok` 还是 `Err` 都调
>   `recorder.drain()`** 取走全部已发生的调用，写入 usage_hourly（§8）。因此
>   `Err(FusionError)` 场景的已付费调用也被统计。
> - **流式尾调用**：返回 `Stream` 时，最终**成功 `Started`** 那次调用的用量随流的
>   `Done` 事件晚到（见 §6.1），由出口层在流关闭后追加；流中途 `Error` 终止时出口层补记
>   一条 `Failed`。即 recorder 收集"所有非流式调用 + 流式 Started 前失败的尝试"，
>   Done/Error 只补"最终那条成功建立的流"。三者无重叠：失败的流式尝试进 recorder，
>   成功建立的流只通过 Done/Error 记一次。
> - `UnifiedResponse.calls` 仅作为成功响应自身的视图（便于 playground 直接展示）；
>   **统计以 `recorder.drain()` + 流式 `Done`/`Error` 为准**，Full 路径只用 recorder，
>   不再额外读 `UnifiedResponse.calls`（避免重复写入）。

> **模型 id 解析（解决 judge / 工具路由的引用）**：`members` 只含虚拟模型显式配置的
> 成员，而 `params.judge`（synthesize/best-of-n）与 multimodal 的能力路由表引用的
> model id **不要求出现在 members 中**。策略通过 `ctx.resolver.resolve(id)` 把任意
> `models` 表中的 id 解析成 `MemberHandle`（含 Connector + 解密后的 EgressCtx）。
> 启动期与保存虚拟模型时校验：`judge` 与能力路由表里的每个 model id 必须存在于
> `models` 表，否则拒绝（fail-fast，见 §3）。这样 judge 可以是一个不参与 panel 的
> 独立模型，能力路由也能指向任意专用后端。

> **流式下的用量统计**：`UnifiedStream` 的终止 `Done` 事件携带最终 `ModelUsage`
> （定义见 §6.1「UnifiedStream 事件模型」）。出口层转发流给客户端的同时缓存这条用量，
> 待流关闭后写入 usage_hourly（见 §8）。因此无论策略返回 `Stream` 还是 `Full`，统计
> 口径一致。

> **策略调试 trace（支撑 playground）**：正常推理路径只需 `StrategyOutput`；但
> playground（§13.2.6）要还原成员答案、judge 输入输出、speed/cheapest 候选对比、
> failover 尝试链、multimodal 时间线，这些是 `UnifiedResponse.calls` 表达不了的。为此
> 定义可选的 `StrategyTrace`（仅当 `StrategyCtx.trace = Some` 时策略才填充，正常请求为
> `None`，零开销）：
>
> ```rust
> pub struct StrategyTrace { /* 内部 Mutex<TraceData> */ }
> impl StrategyTrace {
>     pub fn set_status(&self, status: &str);                 // full|degraded|stop|ok
>     pub fn add_member_answer(&self, model_id: &str, text: &str, usage: &ModelUsage);
>     pub fn set_judge(&self, input: &str, output: &str, usage: &ModelUsage);
>     pub fn add_candidate(&self, model_id: &str, metric: serde_json::Value); // speed吞吐/cheapest成本
>     pub fn add_attempt(&self, model_id: &str, ok: bool, error: Option<&str>); // failover尝试链
>     pub fn add_turn(&self, turn: serde_json::Value);        // multimodal 每轮: 主模型输出/tool call/路由/回填
> }
> ```
>
> 契约：
> - 每个策略只填与自己相关的字段（panel 类填 member_answer + judge + status；
>   failover 填 attempt 链；speed/cheapest 填 candidate 对比；multimodal 填 turn 时间线）。
> - playground 端点构造带 `trace=Some` 的 `StrategyCtx` 跑策略，结束后把 trace 序列化进
>   响应的 `detail` 字段（§13.2.6 的 JSON）；正常 `/v1/*` 推理入口 `trace=None`，不产生
>   任何额外开销。
> - trace 内容仅用于本地调试展示，**不落 usage_hourly**（统计仍走 recorder + 流式 Done）。

- **三层正交**：入口 / 策略 / 出口互不知道对方。
- **流式由策略自报**：单模型类策略返回 `Stream` 走真流；panel 类返回 `Full`，出口层
  按 `want_stream` 包成伪流 SSE。落地「能真流则真流，否则伪流」。
- **策略读 DB**：speed/cheapest 通过 `StrategyCtx.db` 查表决策，决策即普通 SQL。
- **策略注册表**：`strategy/mod.rs` 维护 `name → Box<dyn Strategy>`，加新策略只需实现
  trait + 注册一行。

---

## 7. 六个策略的编排逻辑

### 7.1 failover（故障转移）— 单模型, 真流式
- 按 members position 顺序逐个尝试，第一个成功即返回。
- 失败判定：连接错误 / 5xx / 超时（params `timeout_secs`）。失败记 error 日志，下一个。
- **每个失败的尝试都向 `recorder.record()` 推一条 `status=Failed` 的 `ModelUsage`**
  （流式尝试在 `Started` 前失败同样记，见 §6.3），保证失败尝试进统计。
- 全部失败 → 返回最后一个错误。
- 流式边界：**只在「建连 + 首个事件（`Started`）成功」之前可故障转移**；`Started` 之后
  中断直接报错（token 已发给客户端，无法回退）。

### 7.2 speed（最快）— 单模型, 真流式
- 查 latency_samples：每个 member 取最近 10 条 `AVG(throughput)`，选最高者单模型调用。
- 无样本 member 给乐观默认值让其有机会被探测（params `explore` 可调）。
- 每次真实请求结束写一条 latency_sample（throughput, is_probe=0）。
- 后台探测任务（tokio interval）：对最近 N 分钟无样本的 member 发小探测请求，
  写 is_probe=1 样本，保持数据新鲜。

### 7.3 cheapest（最便宜）— 单模型, 真流式
- 候选 join prices，按 `price_in×估算输入 + price_out×估算输出` 选最低。
- 输入 tokens 用请求内容估算（字符数/4 近似或 tokenizer，params `tokenizer` 可调），
  输出用 max_tokens 或历史均值。
- 缺价格 member：记 warning，排到最后（仅在在价者全不可用时用）。

### 7.4 synthesize（并行合成）— panel 类, 伪流
- 并行（`futures::join_all`）调所有 members `complete`，收集成功答案。
- params.judge 指定真实模型，喂合成 prompt（参考 OpenPrism `research_synthesis`：
  列出各答案，要求 judge 找共识/矛盾/盲点后产出一份）。
- diversity 分级（参考 OpenPrism）：full / degraded（部分失败）/ stop（可用答案过少）；
  params `min_answers`，低于阈值且 `strict=true` 则报错。
- 全部 member 失败 → 报错；judge 失败 → 报错但日志注明 panel 已成功（不丢失已付费工作）。

### 7.5 best-of-n（选最优）— panel 类, 伪流
- 同 synthesize 并行收答案，judge prompt 改为「选最强一份并修复」（参考 OpenPrism
  `code_selection`，best-of-N），输出选中答案 + `Verify by:` 行。params.judge 同上。

### 7.6 multimodal（主模型 + 工具拦截回填）— agentic loop
- members[0] 是主推理模型；params 是能力路由表
  `{web_search: model_id, image_generation: model_id, tool_search:..., image_query:...}`。
- 循环：
  1. 调主模型（带 tools 定义）。
  2. 返回含 ToolCall → 按能力路由表转发到专门后端执行 → 拿 ToolResult。
  3. ToolResult 追加为新 Item，回步骤 1。
  4. 主模型不再发工具调用 → 结束返回最终回答。
- 安全：工具回合后端只做受限能力，不暴露任意 bash/edit（参考 OpenPrism judge 隔离）。
- `max_iterations` 防死循环（params 可配，默认 6）。
- 流式：中间工具回合静默不吐，最后一轮主模型回答可真流。

---

## 8. 用量统计（小时聚合 + 三维度累计）

`request_log` 保留逐条明细（playground/排错）；**累计历史用 `usage_hourly` 预聚合**，
避免每次展示扫全表。

每次请求结束，出口层汇总该请求的**全部底层调用用量**——来源是
`recorder.drain()`（所有非流式调用，含失败）**加上**流式那一次的尾用量
（`UnifiedStream` 的 `Done.usage`，或中途 `Error` 补记的一条 `Failed`）。这份合并后的
`Vec<ModelUsage>`（记作 `all_calls`）是统计的唯一权威来源，不读 `UnifiedResponse.calls`
（后者仅作成功响应自身视图，见 §6.3）。对当前整点 `hour_ts` 做原子 upsert，写三个维度：

- `scope='real'`：遍历 `all_calls`，每条 `ModelUsage` 按其 `model_id` 累加一行——
  累加该条的 token/cost；`requests` **按底层调用计数**（每条 +1）；`status=Failed` 的
  调用累加 `errors`。
- `scope='virtual'`：该**虚拟模型名**累加一行，token/cost 用 `all_calls` 的合计
  （= 各条之和，**不依赖 `UnifiedResponse`**，故错误路径无响应体时同样可累计）；
  `requests` **按对外请求计数**（每个外部请求恰好 +1）；本次请求整体失败（策略返回
  `Err`，或流式以 `Error` 终止）才 `errors` +1。
- `scope='total'`：全局累加一行（name 留空），口径同 virtual（按对外请求计数）。

> **错误路径累计**：策略返回 `Err(FusionError)` 时没有 `UnifiedResponse`，但
> `recorder.drain()` 仍持有已发生的失败/成功调用，`all_calls` 据此构造；virtual/total
> 的 token/cost 用 `all_calls` 合计（可能全为 0），`requests` 照常 +1、`errors` +1。
> 因此「全部 member 失败」「judge 失败」「流式 failover 全部 Started 前失败」等场景都被
> 计入，不会丢请求计数。

**口径说明（重要）**：
- **token / cost**：`real` 各行之和 == `virtual` == `total` 的增量（同一份 `all_calls`
  的不同聚合视角，恒等）。
- **requests**：口径**不同且不可比**——`real.requests` 是底层供应商调用次数（panel/
  multimodal 一个对外请求会产生多次底层调用，故 `real.requests` 之和 ≥
  `virtual.requests`）；`virtual.requests` / `total.requests` 是对外请求次数。前端
  展示时分别标注「底层调用数」与「请求数」，不做跨 scope 的 requests 相等假设。
- **errors**：`real.errors` 计失败的底层调用；`virtual/total.errors` 计整体失败的
  对外请求。同样不跨 scope 相等。

写入：
```sql
INSERT INTO usage_hourly(hour_ts,scope,name,requests,input_tokens,output_tokens,total_tokens,cost,errors)
VALUES(?,?,?,1,?,?,?,?,?)
ON CONFLICT(hour_ts,scope,name) DO UPDATE SET
  requests=requests+1,
  input_tokens=input_tokens+excluded.input_tokens,
  output_tokens=output_tokens+excluded.output_tokens,
  total_tokens=total_tokens+excluded.total_tokens,
  cost=cost+excluded.cost,
  errors=errors+excluded.errors;
```
整点切换自然产生新行，无需定时任务。`cost` 依赖 prices 表，缺价格的真实模型 cost
记 0 并在前端标注「价格未知」。

---

## 9. 日志系统

- 基于 `tracing` + `tracing-subscriber` + `tracing-appender`（文件输出）。
- 三档级别：`debug` / `info` / `error`。
- 可配置项（存 settings 表，Web 界面可改）：
  - `log_level`：debug | info | error
  - `log_file`：日志文件路径（空 = 不写文件）
  - `log_to_stdout`：是否同时输出控制台（默认 true）
- **动态生效**：用 `tracing_subscriber` 的 `reload::Handle` 持有可重载 filter，
  界面改 `log_level` 时热重载，无需重启。文件路径改动需重启（appender 启动时绑定），
  界面提示「修改日志文件需重启」。
- 冷启动：空库用默认值（info / stdout / 无文件）。**首启的 admin token 仅经一次性
  `println!` 直接写控制台，绝不经过 tracing / file appender**（否则会落入日志文件，
  违反 §5.3）。即便此刻日志已初始化，token 输出也走独立的直接 stdout 通道。
- 安全：debug 档可记录请求/响应内容，但**绝不记录**任何密钥明文（见 §5.3）。

---

## 10. 错误处理

统一错误类型 `FusionError`（thiserror），按语义分类并映射 HTTP 状态：

| 变体 | 含义 | 状态 |
|---|---|---|
| `InvalidRequest` | 入口解析/校验失败 | 400 |
| `Unauthorized` | ingress key 无效 / ACL 拒绝 / admin token 无效 | 401/403 |
| `UpstreamError` | 供应商错误（含状态码） | 502 |
| `AllMembersFailed` | 所有成员失败 | 502 |
| `StrategyError` | judge 失败等 | 502 |
| `Internal` | DB / 解密失败 | 500 |

- **出口错误按入口协议格式化**：OpenAI 入口返回 OpenAI 风格 error JSON，Anthropic 入口
  返回 Anthropic 风格，Responses 入口返回 Responses 风格。错误也协议保真。
- 错误不泄密（见 §5.3）。

---

## 11. 测试策略

- **单元测试**：三 connector 请求/响应转换（参考 llm-switch fixtures，jsonl 样本对比）、
  三入口翻译往返、加密往返、speed/cheapest 选择 SQL。
- **策略测试**：mock Connector（不打真网）验证编排——failover 跳过失败者、synthesize
  并行 + judge 调用次数、multimodal loop 终止、speed 按 DB 样本选择。
- **集成测试**：`wiremock` 起假供应商，端到端 axum → 策略 → connector → 假后端，
  覆盖流式/非流式、伪流 SSE 格式。
- **测试数据**：seed 脚本灌入 models / virtual_models / prices / latency_samples
  测试数据（价格抓取程序 v1 不做）。
- 验证门槛：每次改动 `cargo build` + `cargo test` 通过；clippy 干净。

---

## 12. 项目结构

```
localfusion/
├─ Cargo.toml
├─ src/
│  ├─ main.rs              # 启动: 解析 --db、冷启动引导、起监听
│  ├─ db/                  # SQLite: 连接池、迁移、各表查询
│  │  ├─ mod.rs  schema.rs  models.rs  keys.rs  latency.rs  prices.rs  usage.rs
│  ├─ crypto.rs            # ChaCha20-Poly1305 + machine-id KDF + salt
│  ├─ unified.rs           # UnifiedRequest/Response/Item 数据模型
│  ├─ ingress/             # 入口: 三协议解析 → UnifiedRequest, 出口反向
│  │  ├─ openai_chat.rs  openai_responses.rs  anthropic.rs  sse.rs
│  ├─ connector/           # 出口: 三 connector + SSE 翻译 (参考 llm-switch 重写)
│  │  ├─ mod.rs  chat.rs  anthropic.rs  responses.rs
│  ├─ router.rs            # virtual_model → strategy + members + EgressCtx 组装
│  ├─ strategy/            # 策略插件, 每个一文件, trait 注册表
│  │  ├─ mod.rs  failover.rs  speed.rs  cheapest.rs
│  │  ├─ synthesize.rs  best_of_n.rs  multimodal.rs
│  ├─ auth.rs              # ingress key 校验 + ACL + admin token
│  ├─ probe.rs             # speed 定期探测后台任务
│  ├─ logging.rs           # tracing + reload handle
│  ├─ admin/               # 管理 REST API (Spec A) + 内嵌前端 serve (Spec B)
│  │  ├─ api.rs  static.rs
│  └─ error.rs             # FusionError
├─ web/                    # 前端源码 (Spec B), build 产物嵌入二进制
└─ tests/                  # 集成测试 + fixtures
```

依赖：axum、tokio、reqwest、serde/serde_json、sqlx 或 rusqlite、chacha20poly1305、
hkdf、sha2、machine-uid、tracing、tracing-subscriber、tracing-appender、thiserror、
async-trait、futures、rust-embed；dev: wiremock。

---

## 13. Spec B：管理前端

**界面设计与工程范式参考 `../3rd/shadcn-admin`**（Vite + ShadcnUI 后台模板），
沿用其技术栈、目录组织与组件模式，仅替换业务 feature。

**技术栈**（对齐 shadcn-admin）：
- React 19 + TypeScript + Vite + TailwindCSS v4 + shadcn/ui（Radix 原语）。
- 路由：**TanStack Router**（文件式路由）。
- 数据获取：**TanStack Query**（缓存 + 失效重取）；HTTP 用 **axios** 实例（注入 token）。
- 表格：**TanStack Table** + 共享 `components/data-table/`（toolbar / 分页 /
  faceted-filter / column-header / view-options）。
- 表单：**react-hook-form** + **zod**（`@hookform/resolvers`）。
- 全局状态：**zustand**（认证 token、主题）。
- 图标：**lucide-react**；图表：**recharts**；通知：**sonner**。
- 成员排序用上移/下移按钮（不引入 dnd-kit，依赖集与 shadcn-admin 一致）。

`pnpm build` 产物纯静态，用 `rust-embed` 编译期嵌入，axum admin 服务从内存
serve。dist 缺失时回退占位页，保证 Rust 核心可独立编译。

**鉴权流程**：登录页输入 admin token → axios 实例在请求头注入
`Authorization: Bearer <token>` → token 存 zustand + sessionStorage（不落
localStorage）。响应 401 → 清 token 跳登录页。后端校验 admin token 哈希。

### 13.1 管理 REST API（属于 Spec A，前端消费）

```
# 真实模型
GET/POST/PUT/DELETE  /admin/api/models[/:id]      # api_key 提交即加密存, 不回显
                                                  # DELETE 前做完整引用检查(member/judge/能力路由), 有引用返回 409
# 虚拟模型
GET/POST/PUT/DELETE  /admin/api/virtual-models[/:name]
GET                  /admin/api/strategies        # 策略列表 + params schema (驱动动态表单)
# 密钥 / ACL
GET/POST/DELETE      /admin/api/keys[/:id]         # 生成时仅一次返回明文
PATCH                /admin/api/keys/:id           # 改 enabled / label (不涉及 key 本身)
PUT                  /admin/api/keys/:id/acl       # 设置虚拟模型白名单 (acl_all 或具体名单)
# 监控
GET  /admin/api/stats/latency?model=&window=       # 吞吐趋势
GET  /admin/api/stats/prices
GET  /admin/api/stats/requests?...                 # request_log 查询
GET  /admin/api/stats/usage?scope=&name=&from=&to=&granularity=hour|day|week
GET  /admin/api/stats/usage/summary                # 各维度累计总量
GET  /admin/api/health
# 日志配置
GET/PUT  /admin/api/settings/logging               # level 热重载, file 提示需重启
# playground
POST /admin/api/playground                         # 对某虚拟模型发测试请求, 返回编排细节
```

### 13.2 功能面（细化到可执行，套用 shadcn-admin feature 范式）

> 共 6 个页面：登录、真实模型、虚拟模型、密钥/ACL、监控面板、调试台，外加设置/日志。
> 前四个对应你最初要的「四个功能面」（模型/策略配置拆为真实模型 + 虚拟模型两页）。

**通用 feature 范式**（每个功能面 = 一个 `src/features/<名>/`）：
- `index.tsx`：页面骨架，`<Header>`（含 ThemeSwitch + 全局 Search 命令菜单 +
  ProfileDropdown）+ `<Main>` + feature 的 `<XxxProvider>` 包裹 + `<XxxDialogs>`。
- `components/`：`xxx-table.tsx`（TanStack Table）、`xxx-columns.tsx`（列定义 +
  `column-header` 排序 + `row-actions`）、`xxx-dialogs.tsx`（集中渲染增删改对话框）、
  `xxx-provider.tsx`（zustand/context 管理「当前选中行 + 打开哪个对话框」open state）、
  `xxx-primary-buttons.tsx`（右上角「新建」等主操作）、`xxx-mutate-drawer.tsx` 或
  `-action-dialog.tsx`（创建/编辑表单）。
- `data/schema.ts`：zod schema（行类型 + 表单校验）；`data/data.tsx`：静态选项
  （connector/strategy 的 label+icon+color，供 faceted-filter 与 badge 用）。
- 列表统一用共享 `components/data-table/`：`DataTableToolbar`（搜索框 +
  faceted-filter）、`DataTablePagination`、`DataTableViewOptions`（列显隐）、
  `DataTableColumnHeader`（可排序表头）。数据用 TanStack Query 拉取后传入。
- 表单提交走 TanStack Query 的 `useMutation`，成功后 `invalidateQueries` 刷新列表 +
  `sonner` toast；删除用 `AlertDialog` 二次确认。

**侧边栏导航**（`components/layout/data/sidebar-data.ts`，对应 shadcn-admin 的
navGroups）：
- 分组「配置」：真实模型（icon Server）、虚拟模型（icon Boxes）、密钥与 ACL（icon
  KeyRound）。
- 分组「运维」：监控面板（icon LineChart）、调试台（icon FlaskConical）、
  设置/日志（icon Settings）。
- 顶部 team-switcher 位置改为 LocalFusion 品牌标题；nav-user 改为「admin · 登出」。

路由（TanStack Router 文件式，`routes/_authenticated/`）：`/`(监控)、`/models`、
`/virtual-models`、`/keys`、`/playground`、`/settings`；`routes/(auth)/sign-in.tsx`
为登录页。`_authenticated/route.tsx` 的 `beforeLoad` 校验 zustand 里有无 token，
无则重定向登录页。

#### 13.2.0 登录页 `features/auth/sign-in/`（套用 shadcn-admin sign-in 布局）
- 居中卡片：单输入框（admin token，password 型）+「登录」按钮（react-hook-form + zod）。
- 提交：以该 token 调 `GET /admin/api/health`；200 则写入 zustand + sessionStorage 并
  跳监控页，401 则表单错误提示「token 无效」。
- 已登录（sessionStorage 有 token）路由守卫直接跳过登录页。

#### 13.2.1 真实模型 `features/models/` — 数据源 `GET /admin/api/models`
- `models-table`：TanStack Table 列——id、connector（用 `data/data.tsx` 的彩色 badge：
  chat/anthropic/responses）、base_url、model、密钥状态（api_key_enc 有值 = 绿点
  「已加密存储」/ api_key_env = 「env: NAME」/ 都无 = 红点「未配置」）、`row-actions`
  下拉（编辑 / 删除）。toolbar 提供 id 文本搜索 + connector 的 faceted-filter。
- `models-primary-buttons`：「新建模型」按钮 → 打开 `models-action-dialog`。
- `models-action-dialog`（创建/编辑共用，react-hook-form + zod）字段：
  - `id`（文本，必填，唯一，创建后不可改；编辑时禁用）
  - `connector`（Select：chat | anthropic | responses，必填）
  - `base_url`（文本，必填，URL 校验）
  - 密钥方式（RadioGroup）：直填 api_key（password 输入框）/ 指定 api_key_env（文本）
  - `model`（文本，必填，发给供应商的真实模型名）
  - `anthropic_version`（仅 connector=anthropic 时显示，默认 2023-06-01）
  - `extra`（可选，文本域填 JSON，zod 校验可解析；连接器私有字段如 default_max_tokens）
  - 提交 → `POST/PUT /admin/api/models`；api_key 提交即加密存储，响应不回显；编辑时
    api_key 占位「已设置（留空则不变）」，留空 PUT 不改密钥。
- `models-delete-dialog`：`AlertDialog` 确认。后端删除前做**完整引用检查**——扫描
  所有虚拟模型，若该 model id 被用作 ① `virtual_model_members` 成员、② `params.judge`
  （synthesize/best-of-n）、③ `params` 里 multimodal 能力路由表（web_search /
  image_generation / tool_search / image_query）的任一值，则返回 409 + 引用列表（含
  引用方虚拟模型名 + 引用类型），前端 toast 阻止并列出。仅在零引用时才允许删除，
  避免删掉被能力路由引用的模型后虚拟模型运行时失效。

#### 13.2.2 虚拟模型 `features/virtual-models/` — 数据源 `GET /admin/api/virtual-models`
- `virtual-models-table`：列——name、strategy（彩色 badge）、成员数、`row-actions`。
  toolbar 提供 name 搜索 + strategy 的 faceted-filter。
- `virtual-models-mutate-drawer`（创建/编辑，shadcn `Sheet` 抽屉，字段较多用抽屉更顺）：
  - `name`（文本，必填，唯一，即对外模型名）
  - `strategy`（Select，选项来自 `GET /admin/api/strategies`）
  - **成员列表**：每行一个「真实模型」Select + 上移/下移按钮 + 删除按钮，「添加成员」
    追加一行；行顺序即 position。failover/speed/cheapest 标注「顺序 = 优先级」；
    multimodal 标注「第一行 = 主推理模型」。
  - **策略参数动态表单**：`GET /admin/api/strategies` 对每个策略返回一份 JSON Schema
    （params schema），前端据 schema 渲染控件：
    - `synthesize` / `best-of-n`：`judge`（真实模型 Select，必填）、`min_answers`
      （数字）、`strict`（Switch）
    - `failover`：`timeout_secs`（数字）
    - `speed`：`explore`（Switch，是否给无样本模型机会）、探测间隔（数字，分钟）
    - `cheapest`：`tokenizer`（Select：approx | tiktoken）、输出估算上限（数字）
    - `multimodal`：能力路由表——`web_search` / `image_generation` / `tool_search` /
      `image_query` 各一个真实模型 Select（可留空 = 不支持该能力）、`max_iterations`（数字）
  - 提交 → `POST/PUT /admin/api/virtual-models`，body 含 strategy + params(JSON) +
    members(有序数组)。
- 删除同 13.2.1 模式。

#### 13.2.3 密钥/ACL 管理 `features/keys/` — 数据源 `GET /admin/api/keys`
- `keys-table`：列——label、状态（enabled `Switch`，切换调 `PATCH /admin/api/keys/:id`）、
  创建时间（date-fns 格式化）、ACL 摘要（`acl_all=1` 显示「全部」/ 否则前 N 个虚拟模型名
  + 「…」tooltip）、`row-actions`（编辑 ACL / 改 label / 删除）。绝不显示明文。
- `keys-primary-buttons`：「新建 key」→ 输入 label 的小对话框 → `POST /admin/api/keys`
  → **结果对话框一次性展示明文 key**，「复制」按钮 + 红色提示「关闭后无法再次查看」，
  关闭后 invalidate 刷新。
- `keys-acl-dialog`：RadioGroup 选「允许全部（写 `acl_all=1`）」或「指定白名单」；选
  指定时 Checkbox 列表列出所有虚拟模型名（来自 virtual-models query）。保存 →
  `PUT /admin/api/keys/:id/acl`。
- 改 label / 切换 enabled → `PATCH /admin/api/keys/:id`。
- 删除：`AlertDialog` → `DELETE /admin/api/keys/:id`。

#### 13.2.4 监控面板 `features/dashboard/`（套用 shadcn-admin dashboard 范式）
顶部时间范围选择器（小时 / 天 / 周 + 自定义起止，date-fns + react-day-picker），
内容用 `Tabs` + `Card` 网格，recharts 画图：
- **总用量卡片行**（`GET /admin/api/stats/usage/summary`）：累计请求数、累计
  input/output/total token、累计成本（缺价格部分标注「不含未定价模型」）——对应
  shadcn-admin dashboard 顶部的统计卡片。
- **用量趋势 + 排行**（`GET /admin/api/stats/usage?scope=&granularity=`）：
  - recharts 折线/柱图：按所选粒度的 total token / 成本时间序列（scope=total）。
  - `Tabs` 切 real / virtual：TanStack Table 排行（按 total_token 降序）列出各真实
    模型 / 各虚拟模型的 requests、input/output/total token、cost、errors；行点开看
    其单独趋势。
- **吞吐延迟**（`GET /admin/api/stats/latency?model=&window=`）：每个真实模型最近吞吐
  （tokens/s）折线图，探测样本（is_probe=1）用不同标记点区分真实请求。
- **价格表**（`GET /admin/api/stats/prices`）：只读表格，model_id / price_in /
  price_out / updated_at，updated_at 超阈值（如 7 天）标黄「价格可能过期」。
- **request_log 查询**（`GET /admin/api/stats/requests`）：可按 virtual_name /
  strategy / status / 时间筛选的明细 data-table，分页。

#### 13.2.5 设置/日志 `features/settings/`（套用 shadcn-admin settings 范式）
左侧 `sidebar-nav` 子导航 + 右侧 `content-section`，第一项「日志」：
- 表单（`GET/PUT /admin/api/settings/logging`，react-hook-form）：level Select
  （debug/info/error，保存即热重载）、log_file 文本框（保存提示「需重启生效」）、
  log_to_stdout `Switch`。
- 预留「服务器」子页展示只读的 inference_bind / admin_bind（改需重启）。

#### 13.2.6 调试台 / playground `features/playground/`
- 表单：虚拟模型 Select（来自 virtual-models query）+ 多行 prompt 输入 +「发送」。
- 提交 → `POST /admin/api/playground`（body: virtual_name + prompt）。后端构造一个
  `StrategyCtx { trace: Some(..), recorder, want_stream: false }` 跑一次该虚拟模型的
  策略，把 `recorder.drain()` 合并的用量映射成 `calls`、把 `StrategyTrace` 序列化进
  `detail`，返回：
  ```json
  {
    "final": "最终回答文本",
    "strategy": "synthesize",
    "status": "full|degraded|stop|ok",
    "calls": [ { "model_id","role","input_tokens","output_tokens","cost","status","estimated","latency_secs" } ],
    "detail": { /* StrategyTrace 序列化, 字段随策略而异, 见下 */ }
  }
  ```
  - `calls` 来自合并后的 `all_calls`（`recorder.drain()` + 流式尾用量，含失败调用），
    每条即 §6.1 的 `ModelUsage` 序列化（含 `estimated` / `latency_secs` 字段）。
  - `detail` 来自 §6.3 的 `StrategyTrace`：panel 类含 `member_answers[] + judge{input,
    output} + status`；failover 含 `attempts[]`（尝试链）；speed/cheapest 含
    `candidates[]`（候选指标对比）；multimodal 含 `turns[]`（时间线）。
- 展示按策略类型渲染 `detail`（用 `Card` + `Tabs`）：
  - **panel 类（synthesize/best-of-n）**：左侧并排各成员答案卡片（model + 答案 +
    ok/fail + 用量，来自 `detail.member_answers`），右侧 judge 的合成/选优结果
    （`detail.judge`）；顶部 diversity 分级 badge（`detail.status`）。
  - **单模型类（failover/speed/cheapest）**：显示「被选中模型」+ 选择理由——speed 各
    候选最近吞吐对比、cheapest 各候选成本估算对比（`detail.candidates`）、failover
    尝试链（`detail.attempts`，哪些失败、最终哪个成功）。
  - **multimodal**：时间线视图（`detail.turns`），逐轮展示「主模型输出 → 触发的 tool
    call → 路由到哪个后端 → ToolResult → 回填」，直到终止。
- 底部统一显示本次 `calls` 用量明细表与合计（含失败调用）。

### 13.3 前端结构与构建（套用 shadcn-admin 目录范式）

```
web/
├─ package.json  vite.config.ts  components.json  tsconfig.*.json
├─ index.html
├─ src/
│  ├─ main.tsx                      # 挂载 + QueryClientProvider + RouterProvider
│  ├─ routes/                       # TanStack Router 文件式路由
│  │  ├─ __root.tsx
│  │  ├─ (auth)/sign-in.tsx
│  │  └─ _authenticated/
│  │     ├─ route.tsx               # 守卫: 无 token 跳登录
│  │     ├─ index.tsx               # 监控面板
│  │     ├─ models/index.tsx
│  │     ├─ virtual-models/index.tsx
│  │     ├─ keys/index.tsx
│  │     ├─ playground/index.tsx
│  │     └─ settings/{route,index}.tsx
│  ├─ features/                     # 业务功能 (每个含 index + components/ + data/)
│  │  ├─ auth/  models/  virtual-models/  keys/
│  │  ├─ dashboard/  playground/  settings/
│  ├─ components/
│  │  ├─ ui/                        # shadcn 组件
│  │  ├─ data-table/                # 共享表格: toolbar/pagination/column-header/...
│  │  └─ layout/                    # app-sidebar/header/main/nav-* (改造自参考)
│  ├─ lib/  api.ts (axios 实例+token 注入)  utils.ts
│  ├─ stores/  auth-store.ts (zustand)
│  ├─ hooks/  use-table-url-state.ts  ...
│  └─ config/  fonts/styles
└─ dist/                            # build 产物 → rust-embed 嵌入
```

前端依赖（对齐 shadcn-admin，见 §13 技术栈）：react 19、@tanstack/react-router、
@tanstack/react-query、@tanstack/react-table、axios、react-hook-form、zod、
@hookform/resolvers、zustand、recharts、lucide-react、sonner、date-fns、
react-day-picker、tailwindcss v4 + shadcn/ui（Radix）。**不含** dnd-kit、clerk
（认证用我们自己的 admin token，不引入 Clerk）。

构建：`cd web && pnpm install && pnpm build`（`tsc -b && vite build`），再
`cargo build --release`。CI 两步串联。`rust-embed` 在 `dist/` 缺失时回退占位页
（仅显示「前端未构建」），保证 Rust 核心可独立编译。

---

## 14. 实现顺序

1. **Spec A**：Rust 核心全部完成（含管理 REST API），用 curl 可全量管理并测试通过。
2. **Spec B**：React 前端，消费已就绪的管理 API，嵌入二进制。

产物自始至终是一个独立的 Rust 可执行文件。



