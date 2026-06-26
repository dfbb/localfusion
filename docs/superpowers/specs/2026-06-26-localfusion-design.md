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
  3. 生成一个随机 **admin token**，**打印到控制台一次**（只此一次，存哈希），用户拿它
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
  created_at INTEGER NOT NULL
);
CREATE TABLE ingress_key_acl (
  key_id       INTEGER NOT NULL REFERENCES ingress_keys(id) ON DELETE CASCADE,
  virtual_name TEXT NOT NULL,        -- '*' = 全部
  PRIMARY KEY (key_id, virtual_name)
);

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

- **speed 查询**：`SELECT AVG(throughput) FROM latency_samples WHERE model_id=?
  ORDER BY created_at DESC LIMIT 10` → 取候选最近 10 次平均吞吐，选最高。
- **cheapest 查询**：候选 join prices，按 `price_in×估算输入 + price_out×估算输出`
  选最低；缺价格者记 warning 排到最后。
- **价格/延迟解耦**：prices 由第三方写、localfusion 只读；latency 由 localfusion 自己
  写（真实请求 + 探测）。价格抓取程序 v1 不开发，测试用 seed 数据填充。

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
  校验哈希；并检查该 key 的 ACL 白名单是否允许其请求的虚拟模型（`*`=全部）。
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

/// 统一响应。策略产出它, 出口翻译回目标协议。
pub struct UnifiedResponse {
    pub items: Vec<Item>,    // 通常是 assistant Message, 可能含 ToolCall
    pub usage: Usage,        // input/output tokens (cheapest / 统计用)
    pub model_id: String,    // 实际生效的真实模型 (panel类=judge, 单模型类=被选中者)
}
```

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

### 6.3 Strategy trait（编排器，每策略一插件）

```rust
#[async_trait]
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, ctx: StrategyCtx<'_>) -> Result<StrategyOutput, FusionError>;
}

pub struct StrategyCtx<'a> {
    pub req: UnifiedRequest,
    pub members: Vec<MemberHandle<'a>>,  // 每个含 Connector + EgressCtx
    pub params: serde_json::Value,       // 该虚拟模型的策略私有参数
    pub db: &'a Db,                      // speed/cheapest 查延迟/价格表
    pub want_stream: bool,
}

pub enum StrategyOutput {
    Stream(UnifiedStream),    // 真流式: 某单模型原生流直接透传
    Full(UnifiedResponse),    // 完整响应: 出口层据 want_stream 决定是否切伪流 SSE
}
```

性质：

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
- 全部失败 → 返回最后一个错误。
- 流式边界：**只在「建连 + 首个事件成功」之前可故障转移**；首个事件之后中断直接报错
  （token 已发给客户端，无法回退）。

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

每次请求结束，对当前整点 `hour_ts` 做原子 upsert，写三个维度：

- `scope='real'`：每个**实际被调用的真实模型**各累加一行（panel 策略一次请求涉及
  多个真实模型 + judge，各自累加自己的 token）。
- `scope='virtual'`：该**虚拟模型名**累加一行（聚合本次所有底层调用总 token）。
- `scope='total'`：全局累加一行（name 留空）。

口径自洽：real 是底层调用视角，virtual 是对外请求视角，total 是全局。

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
- 冷启动：空库用默认值（info / stdout / 无文件），首启日志正常打印（含 admin token）。
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

**技术栈**：React + TypeScript + Vite + Tailwind + shadcn/ui。`npm build` 产物纯静态，
用 `rust-embed` 编译期嵌入，axum admin 服务从内存 serve。dist 缺失时仍能编译（开发
Rust 核心不必装 node）。

**鉴权流程**：登录页输入 admin token → 调管理 API 带 `Authorization: Bearer <token>`
→ token 存内存/sessionStorage，不落 localStorage。后端校验 admin token 哈希。

### 13.1 管理 REST API（属于 Spec A，前端消费）

```
# 真实模型
GET/POST/PUT/DELETE  /admin/api/models[/:id]      # api_key 提交即加密存, 不回显
# 虚拟模型
GET/POST/PUT/DELETE  /admin/api/virtual-models[/:name]
GET                  /admin/api/strategies        # 策略列表 + params schema (驱动动态表单)
# 密钥 / ACL
GET/POST/DELETE      /admin/api/keys[/:id]         # 生成时仅一次返回明文
PUT                  /admin/api/keys/:id/acl       # 设置虚拟模型白名单
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

### 13.2 四个功能面

1. **模型/策略配置**：真实模型增删改（api_key 只写不回显，编辑显示「已设置」占位）；
   虚拟模型选 strategy 后据 params schema **动态渲染参数表单**（synthesize/best-of-n
   显示 judge 选择器，multimodal 显示能力路由表，其余显示各自参数）；members 可拖拽
   排序（position 即顺序）。
2. **密钥/ACL 管理**：key 列表（label/启用/创建时间，不显明文）；新建弹窗一次性展示
   明文；每 key 的 ACL 多选虚拟模型白名单，支持 `*`。
3. **状态/监控面板**：
   - 各真实模型最近吞吐（tokens/s）趋势图（含探测点标记）。
   - 价格表只读 + updated_at 新鲜度。
   - **完整 token 调用累计**：总用量卡片（累计请求/token/成本）、按真实模型与按虚拟
     模型的用量排行 + 时间趋势图（基于 usage_hourly，可切小时/天/周）。
   - request_log 可筛选查询。
4. **在线调试 / playground**：选虚拟模型 + prompt → 发请求 → 展示最终回答 + **策略
   编排细节**：panel 类显示各成员答案 + judge 合成 + diversity 分级；单模型类显示选中
   模型及理由（speed 吞吐 / cheapest 成本估算 / failover 尝试链）；multimodal 显示
   工具调用 → 路由 → 回填每步。

### 13.3 前端结构与构建

```
web/
├─ package.json  vite.config.ts  tailwind.config.ts
├─ src/
│  ├─ main.tsx  App.tsx  router.tsx
│  ├─ lib/api.ts          # 封装 admin API + token 注入
│  ├─ components/ui/      # shadcn 组件
│  ├─ pages/  Login Models VirtualModels Keys Monitor Playground
│  └─ hooks/  types/
└─ dist/                  # build 产物 → rust-embed 嵌入
```

构建：`cd web && npm install && npm run build`，再 `cargo build --release`。
CI 两步串联。

---

## 14. 实现顺序

1. **Spec A**：Rust 核心全部完成（含管理 REST API），用 curl 可全量管理并测试通过。
2. **Spec B**：React 前端，消费已就绪的管理 API，嵌入二进制。

产物自始至终是一个独立的 Rust 可执行文件。



