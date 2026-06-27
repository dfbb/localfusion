# LocalFusion

本地独立运行的多供应商 LLM 混合调度代理。单一 Rust 可执行文件，对外暴露 **OpenAI / OpenAI Responses / Anthropic** 三套兼容端点,对内按「虚拟模型 → 调度策略 → 真实模型组」扇出到多家供应商,并内置一个嵌入式 React 管理界面。

标准 SDK 零改动即可接入:把 `base_url` 指向 LocalFusion,把 `model` 填成你定义的「虚拟模型名」,剩下的路由、故障转移、合成、计费统计全部由 LocalFusion 负责。

---

## 特性

- **三套兼容入口**:OpenAI Chat Completions、OpenAI Responses、Anthropic Messages,均支持流式(SSE)与非流式。
- **虚拟模型 + 6 种调度策略**:每个虚拟模型名绑定一种策略和一组真实后端模型。
  - `failover` —— 故障转移:按顺序尝试,失败自动切下一个。
  - `speed` —— 最快:按最近吞吐(tokens/s)选当前最快的成员。
  - `cheapest` —— 最便宜:按价格表估算成本,选最低。
  - `synthesize` —— 并行合成:并行调用所有成员,再由 judge 模型合成一份共识答案。
  - `best-of-n` —— 选最优:并行收集后由 judge 选出最强一份并修复。
  - `multimodal` —— 主模型 + 工具拦截回填:agentic loop,主模型的工具调用按能力路由表转发到后端执行并回填。
- **全部配置存 SQLite**:没有 config 文件。模型、虚拟模型、密钥、ACL、价格、日志级别都在数据库里,可经管理 API / 前端热修改。
- **嵌入式管理前端**:React + Vite + TanStack + shadcn,编译产物用 `rust-embed` 嵌进同一个二进制。提供模型管理、虚拟模型编排、密钥/ACL、监控面板、调试台(策略 trace 可视化)、日志设置。
- **三维度用量统计**:按小时预聚合,`real`(真实模型)/ `virtual`(虚拟模型)/ `total`(全局)三个口径,token 与成本据 `CallRecorder` 统计(含失败调用),不依赖响应体。
- **密钥安全**:provider key 用 ChaCha20-Poly1305(密钥经 HKDF-SHA256 从 machine-id 派生)加密落库;ingress key 与 admin token 仅存 SHA-256 哈希;admin token 仅首启打印一次,绝不写日志。
- **单一可执行文件**:前端 + 后端 + 迁移全部打包,部署即一个二进制 + 一个 SQLite 文件。

---

## 架构

三层正交,通过统一中间表示(`UnifiedRequest` / `UnifiedResponse` / `UnifiedStream`)解耦:

```
客户端 SDK
   │  (OpenAI / Responses / Anthropic 协议)
   ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│  入口 Ingress │ → │  策略 Strategy │ → │  出口 Connector│
│  协议解析     │   │  虚拟模型扇出  │   │  调真实供应商  │
└──────────────┘   └──────────────┘   └──────────────┘
   │                      │                    │
   │   鉴权(key+ACL)      │  Router 按         │  ChaCha20 解密 key
   │                      │  strategy 分发     │  SSE 字节级安全切帧
   ▼                      ▼                    ▼
            SQLite(配置 + 小时聚合统计 + 请求明细)
```

- **入口层**只懂协议翻译,不知道下游是谁。
- **策略层**只懂「怎么选/怎么合并成员」,不知道入口协议,也不知道出口怎么发 HTTP。
- **出口层**只懂把统一请求翻译成某家供应商的真实 HTTP 调用,并把响应/SSE 翻译回来。

进程内运行**两个** axum 服务:

| 服务 | 默认绑定 | 用途 |
| --- | --- | --- |
| 推理服务 | `127.0.0.1:8787` | 接受客户端 SDK 请求(三套兼容入口) |
| 管理服务 | `127.0.0.1:8788` | 管理 REST API + 嵌入式前端,admin token 鉴权 |

---

## 环境要求

- **Rust** 稳定版工具链(edition 2021),`cargo` 可用。
- **Node.js** + **pnpm**(仅在需要构建/修改前端时)。仓库前端用 pnpm 管理(`web/pnpm-lock.yaml`)。
- 运行时无需额外服务,SQLite 由 `sqlx` 内嵌驱动,数据库文件首次运行自动创建。

---

## 安装与构建

构建是两步串联:先编译前端产物到 `web/dist`,再 `cargo build` 把它嵌进二进制。

```bash
# 1. 构建前端(产物输出到 web/dist)
cd web
pnpm install
pnpm build
cd ..

# 2. 构建后端(rust-embed 嵌入 web/dist)
cargo build --release
```

产物在 `target/release/localfusion`。

> **关于前端缺失**:`web/dist` 缺失时后端仍可编译(管理服务会返回一个占位页,提示先运行 `pnpm build`)。若只想跑后端核心、用 curl 管理,可跳过第 1 步。

---

## 快速开始

```bash
# 启动(数据库文件不存在会自动创建)
./target/release/localfusion --db ./localfusion.db
```

首次启动时,控制台会**仅打印一次** admin token:

```
=== LocalFusion admin token (save it, shown only once) ===
lfadm-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

**务必保存它**——它只在首启打印一次,之后只存哈希,无法找回。用它登录管理界面或调用管理 API。

启动后:

- 管理界面:浏览器打开 `http://127.0.0.1:8788/`,用 admin token 登录。
- 推理入口:`http://127.0.0.1:8787/v1/...`(见下)。

### 命令行参数

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `--db <PATH>` | `./localfusion.db` | SQLite 数据库文件路径,不存在则创建。 |

绑定地址(`inference_bind` / `admin_bind`)存在数据库的 settings 表,默认 `127.0.0.1:8787` 与 `127.0.0.1:8788`,可经管理 API 修改(改后需重启生效)。

### 优雅关闭

进程收到 `SIGINT`(Ctrl-C)或 `SIGTERM` 时,两个服务停止接收新连接、放行在途请求,后台探测任务退出后干净结束。

---

## 配置流程

LocalFusion 无配置文件,所有配置经管理界面或管理 API 写入数据库。典型首配顺序:

1. **添加真实模型**:登记上游供应商(connector 类型 = `chat` / `anthropic` / `responses`、base_url、密钥、模型名)。密钥可直填(加密落库)或填环境变量名。
2. **创建虚拟模型**:起一个对外的虚拟模型名,选一种策略,挑选成员(真实模型),配置策略参数(如 synthesize/best-of-n 的 judge 模型)。
3. **创建 ingress key**:生成客户端调用用的 API key(明文仅展示一次),并设置 ACL(允许全部虚拟模型,或指定白名单)。
4. **(可选)配置价格表**:供 `cheapest` 策略估算成本、统计计费。

之后客户端用 ingress key 调推理入口、`model` 填虚拟模型名即可。

---

## 使用示例

假设你创建了一个虚拟模型 `my-router`,并生成了 ingress key `sk-lf-xxxx`。

### OpenAI Chat Completions

```bash
curl http://127.0.0.1:8787/v1/chat/completions \
  -H "Authorization: Bearer sk-lf-xxxx" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "my-router",
    "messages": [{"role": "user", "content": "你好"}],
    "stream": false
  }'
```

用官方 OpenAI SDK 时,只需把 `base_url` 指向 `http://127.0.0.1:8787/v1`、`api_key` 用 ingress key、`model` 用虚拟模型名。

### Anthropic Messages

```bash
curl http://127.0.0.1:8787/v1/messages \
  -H "x-api-key: sk-lf-xxxx" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "my-router",
    "messages": [{"role": "user", "content": "你好"}],
    "max_tokens": 256
  }'
```

### OpenAI Responses

```bash
curl http://127.0.0.1:8787/v1/responses \
  -H "Authorization: Bearer sk-lf-xxxx" \
  -H "Content-Type: application/json" \
  -d '{"model": "my-router", "input": "你好"}'
```

三套入口都支持 `stream: true` 的 SSE 流式输出,错误响应按各自协议格式返回。

---

## API 参考

### 推理入口(端口 8787,ingress key 鉴权)

鉴权:`Authorization: Bearer <ingress-key>` 或 `x-api-key: <ingress-key>`。

| 方法 | 路径 | 协议 |
| --- | --- | --- |
| POST | `/v1/chat/completions` | OpenAI Chat Completions |
| POST | `/v1/responses` | OpenAI Responses |
| POST | `/v1/messages` | Anthropic Messages |

### 管理 API(端口 8788,admin token 鉴权)

鉴权:`Authorization: Bearer <admin-token>`。

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/admin/api/health` | 鉴权探活 |
| GET / POST | `/admin/api/models` | 列出 / 新增真实模型 |
| PUT / DELETE | `/admin/api/models/:id` | 修改 / 删除真实模型(删除前做引用检查) |
| GET / POST | `/admin/api/virtual-models` | 列出 / 新增虚拟模型 |
| PUT / DELETE | `/admin/api/virtual-models/:name` | 修改 / 删除虚拟模型 |
| GET | `/admin/api/strategies` | 列出策略及其参数 schema |
| GET / POST | `/admin/api/keys` | 列出 / 新增 ingress key(明文仅返回一次) |
| PATCH / DELETE | `/admin/api/keys/:id` | 启用停用/改标签 / 删除 |
| PUT | `/admin/api/keys/:id/acl` | 设置 key 的 ACL(全部或白名单) |
| GET | `/admin/api/stats/usage` | 小时聚合用量(可按 scope/name/时间范围筛选) |
| GET | `/admin/api/stats/usage/summary` | 用量合计 |
| GET | `/admin/api/stats/prices` | 价格表 |
| GET | `/admin/api/stats/latency` | 各模型最近吞吐(tokens/s) |
| GET | `/admin/api/stats/requests` | 逐条请求明细 |
| POST | `/admin/api/playground` | 调试台:跑一次虚拟模型并返回完整策略 trace |
| GET / PUT | `/admin/api/settings/logging` | 日志级别(热重载)/文件/stdout 设置 |

> 时间戳约定:管理 API 的时间字段为 **Unix 秒**。

---

## 管理界面

浏览器访问管理端口(默认 `http://127.0.0.1:8788/`),用 admin token 登录。包含:

- **真实模型**:增删改上游模型与密钥。
- **虚拟模型**:选策略、编排成员(支持上下移)、动态策略参数表单。
- **密钥 / ACL**:生成 ingress key(明文一次性展示)、启停、设置可访问的虚拟模型范围。
- **监控面板**:总用量 / 趋势折线 / 模型排行 / 延迟 / 价格 / 请求明细。
- **调试台 (Playground)**:对某个虚拟模型发一次请求,可视化策略编排过程(成员答案、judge 输入输出、候选对比、尝试链、multimodal 轮次时间线)。
- **设置**:日志级别(保存即热重载)、日志文件、stdout 开关。

---

## 开发

```bash
# 后端测试(单元 + 集成 + e2e)
cargo test

# Lint
cargo clippy --all-targets

# 前端开发态(Vite dev server,默认 :5173)
cd web && pnpm dev
```

前端开发态运行在另一个 localhost 端口,管理服务已配置 CORS **仅放行 localhost / 127.0.0.1 来源**,因此 dev server 可直接调 `:8788` 的管理 API。

后端测试使用 `wiremock` 起假上游,验证端到端路由、协议翻译、SSE 切帧与统计落库等真实行为。

---

## 安全说明

LocalFusion 的设计定位是**本地单机单用户工具**(默认仅绑定 `127.0.0.1`)。已落实的安全措施:

- provider key 用 ChaCha20-Poly1305 加密落库(密钥经 HKDF-SHA256 从 machine-id 派生,salt 随机)。
- ingress key 与 admin token 仅存 SHA-256 哈希;明文 ingress key 仅在创建时返回一次。
- admin token 仅首启 `println!` 打印一次,绝不写入日志。
- 所有 SQL 参数化,无字符串拼接。
- 推理入口经 key + ACL 鉴权;管理 API 每个端点经 admin token 鉴权。
- 上游错误体截断脱敏后再返回,SSE 出口缓冲有上限,防止异常上游撑爆内存。

> 若要暴露到非 `127.0.0.1` 的地址(改 `inference_bind` / `admin_bind`),请自行评估网络层防护——该工具未针对公网多租户场景加固。

---

## 许可证

本项目采用 [Apache License 2.0](LICENSE) 授权。

