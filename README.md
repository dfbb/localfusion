# LocalFusion

A locally self-hosted multi-provider LLM routing proxy. A single Rust binary that exposes three compatible endpoint suites — **OpenAI / OpenAI Responses / Anthropic** — and internally fans out requests to multiple providers via a "virtual model → dispatch strategy → real model group" pipeline, with an embedded React admin UI.

Drop-in compatible with standard SDKs: point `base_url` at LocalFusion, set `model` to your virtual model name, and LocalFusion handles all routing, failover, synthesis, and billing tracking.

---

## Features

- **Three compatible ingress endpoints**: OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages — all supporting both streaming (SSE) and non-streaming.
- **Virtual models + 6 dispatch strategies**: each virtual model name is bound to one strategy and a set of real backend models.
  - `failover` — tries members in order; automatically falls back to the next on failure.
  - `speed` — selects the currently fastest member based on recent throughput (tokens/s).
  - `cheapest` — estimates cost from the price table and selects the lowest-cost option.
  - `synthesize` — parallel synthesis: calls all members concurrently, then uses a judge model to synthesize a consensus answer.
  - `best-of-n` — parallel collection followed by a judge selecting and refining the best response.
  - `multimodal` — primary model with tool-call interception and backfilling: an agentic loop that routes the primary model's tool calls to backends based on a capability routing table.
- **All configuration stored in SQLite**: no config files. Models, virtual models, keys, ACLs, prices, and log levels live in the database and can be hot-modified via the admin API or UI.
- **Embedded admin frontend**: React + Vite + TanStack + shadcn, compiled and embedded into the same binary via `rust-embed`. Includes model management, virtual model orchestration, key/ACL management, a monitoring dashboard, a debug playground (strategy trace visualization), and log settings.
- **Three-dimensional usage statistics**: pre-aggregated by hour across three scopes — `real` (per real model), `virtual` (per virtual model), and `total` (global) — with token and cost counts recorded by `CallRecorder` (including failed calls), independent of response bodies.
- **Key security**: provider keys are encrypted at rest with ChaCha20-Poly1305 (key derived from machine-id via HKDF-SHA256 with a random salt); ingress keys and the admin token are stored only as SHA-256 hashes; the admin token is printed once on first startup and never written to logs.
- **Single binary**: frontend, backend, and migrations are all bundled together — deployment is one binary plus one SQLite file.

---

## Architecture

Three orthogonal layers, decoupled through a unified intermediate representation (`UnifiedRequest` / `UnifiedResponse` / `UnifiedStream`):

```
Client SDK
   │  (OpenAI / Responses / Anthropic protocol)
   ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│  Ingress     │ → │  Strategy    │ → │  Connector   │
│  Protocol    │   │  Virtual     │   │  Real        │
│  parsing     │   │  model fanout│   │  provider    │
└──────────────┘   └──────────────┘   └──────────────┘
   │                      │                    │
   │   Auth (key+ACL)     │  Router dispatches │  ChaCha20 key decryption
   │                      │  by strategy       │  SSE byte-safe framing
   ▼                      ▼                    ▼
            SQLite (config + hourly aggregated stats + request detail)
```

- The **ingress layer** only understands protocol translation; it has no knowledge of downstream targets.
- The **strategy layer** only understands how to select/merge members; it has no knowledge of the ingress protocol or how the connector makes HTTP calls.
- The **connector layer** only understands how to translate a unified request into a real HTTP call for a specific provider, and translate the response/SSE back.

Two axum servers run in-process:

| Server | Default bind | Purpose |
| --- | --- | --- |
| Inference server | `127.0.0.1:8787` | Accepts client SDK requests (three compatible ingress endpoints) |
| Admin server | `127.0.0.1:8788` | Admin REST API + embedded frontend, admin token auth |

---

## Requirements

- **Rust** stable toolchain (edition 2021) with `cargo` available.
- **Node.js** + **pnpm** (only needed to build or modify the frontend). The frontend uses pnpm (`web/pnpm-lock.yaml`).
- No external services required at runtime. SQLite is driven by the embedded `sqlx` driver; the database file is created automatically on first run.

---

## Installation & Build

Building is a two-step pipeline: first compile the frontend output to `web/dist`, then `cargo build` embeds it into the binary.

```bash
# 1. Build the frontend (output goes to web/dist)
cd web
pnpm install
pnpm build
cd ..

# 2. Build the backend (rust-embed bundles web/dist)
cargo build --release
```

The output binary is at `target/release/localfusion`.

> **If the frontend is missing**: when `web/dist` is absent the backend still compiles (the admin server will return a placeholder page instructing you to run `pnpm build` first). If you only want to run the backend core and manage it via curl, you can skip step 1.

---

## Quick Start

```bash
# Start (database file is created automatically if it doesn't exist)
./target/release/localfusion --db ./localfusion.db
```

On first startup, the console prints the admin token **exactly once**:

```
=== LocalFusion admin token (save it, shown only once) ===
lfadm-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

**Save it** — it is printed only on the very first startup. After that, only the hash is stored and it cannot be recovered. Use it to log in to the admin UI or call the admin API.

After startup:

- Admin UI: open `http://127.0.0.1:8788/` in your browser and log in with the admin token.
- Inference endpoint: `http://127.0.0.1:8787/v1/...` (see below).

### CLI Arguments

| Argument | Default | Description |
| --- | --- | --- |
| `--db <PATH>` | `./localfusion.db` | Path to the SQLite database file; created if it does not exist. |

Bind addresses (`inference_bind` / `admin_bind`) are stored in the database settings table, defaulting to `127.0.0.1:8787` and `127.0.0.1:8788`. They can be changed via the admin API (restart required to take effect).

### Graceful Shutdown

When the process receives `SIGINT` (Ctrl-C) or `SIGTERM`, both servers stop accepting new connections, allow in-flight requests to complete, and exit cleanly once background probe tasks have terminated.

---

## Configuration Workflow

LocalFusion has no config file; all configuration is written to the database via the admin UI or admin API. Typical first-time setup order:

1. **Add real models**: register upstream providers (connector type = `chat` / `anthropic` / `responses`, base_url, API key, model name). Keys can be entered directly (encrypted at rest) or as environment variable names.
2. **Create virtual models**: define a virtual model name, choose a strategy, select member real models, and configure strategy parameters (e.g., the judge model for `synthesize`/`best-of-n`).
3. **Create an ingress key**: generate an API key for client use (plaintext shown once), and configure its ACL (allow all virtual models, or specify a whitelist).
4. **(Optional) Configure the price table**: used by the `cheapest` strategy for cost estimation and billing statistics.

Clients then call the inference endpoint with the ingress key and the virtual model name as the `model` field.

---

## Usage Examples

Assuming you have created a virtual model `my-router` and generated an ingress key `sk-lf-xxxx`.

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

When using the official OpenAI SDK, simply set `base_url` to `http://127.0.0.1:8787/v1`, `api_key` to your ingress key, and `model` to your virtual model name.

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

All three endpoints support `stream: true` SSE streaming output. Error responses follow each protocol's respective format.

---

## API Reference

### Inference Endpoints (port 8787, ingress key auth)

Auth: `Authorization: Bearer <ingress-key>` or `x-api-key: <ingress-key>`.

| Method | Path | Protocol |
| --- | --- | --- |
| POST | `/v1/chat/completions` | OpenAI Chat Completions |
| POST | `/v1/responses` | OpenAI Responses |
| POST | `/v1/messages` | Anthropic Messages |

### Admin API (port 8788, admin token auth)

Auth: `Authorization: Bearer <admin-token>`.

| Method | Path | Description |
| --- | --- | --- |
| GET | `/admin/api/health` | Authenticated health check |
| GET / POST | `/admin/api/models` | List / create real models |
| PUT / DELETE | `/admin/api/models/:id` | Update / delete real model (reference check before deletion) |
| GET / POST | `/admin/api/virtual-models` | List / create virtual models |
| PUT / DELETE | `/admin/api/virtual-models/:name` | Update / delete virtual model |
| GET | `/admin/api/strategies` | List strategies and their parameter schemas |
| GET / POST | `/admin/api/keys` | List / create ingress keys (plaintext returned once only) |
| PATCH / DELETE | `/admin/api/keys/:id` | Enable/disable, rename, or delete a key |
| PUT | `/admin/api/keys/:id/acl` | Set key ACL (all virtual models or a whitelist) |
| GET | `/admin/api/stats/usage` | Hourly aggregated usage (filterable by scope/name/time range) |
| GET | `/admin/api/stats/usage/summary` | Usage totals |
| GET | `/admin/api/stats/prices` | Price table |
| GET | `/admin/api/stats/latency` | Recent throughput (tokens/s) per model |
| GET | `/admin/api/stats/requests` | Per-request detail log |
| POST | `/admin/api/playground` | Debug playground: run a virtual model and return the full strategy trace |
| GET / PUT | `/admin/api/settings/logging` | Log level (hot-reload) / file / stdout settings |

> Timestamp convention: time fields in the admin API are **Unix seconds**.

---

## Admin UI

Open the admin port in your browser (default `http://127.0.0.1:8788/`) and log in with the admin token. Sections include:

- **Real Models**: add, edit, and delete upstream models and keys.
- **Virtual Models**: choose a strategy, arrange members (drag to reorder), and configure dynamic strategy parameter forms.
- **Keys / ACL**: generate ingress keys (plaintext shown once), enable/disable, and set the accessible virtual model scope.
- **Monitoring Dashboard**: total usage, trend charts, model rankings, latency, prices, and request detail.
- **Debug Playground**: send a single request to a virtual model and visualize the full orchestration process (member answers, judge input/output, candidate comparison, attempt chains, multimodal round timeline).
- **Settings**: log level (saved with immediate hot-reload), log file path, and stdout toggle.

---

## Development

```bash
# Backend tests (unit + integration + e2e)
cargo test

# Lint
cargo clippy --all-targets

# Frontend dev mode (Vite dev server, default :5173)
cd web && pnpm dev
```

The frontend dev server runs on a separate localhost port. The admin server is configured to allow CORS **only from localhost / 127.0.0.1 origins**, so the dev server can call the `:8788` admin API directly.

Backend tests use `wiremock` to spin up fake upstream servers, verifying real end-to-end behavior including routing, protocol translation, SSE framing, and stats persistence.

---

## Security Notes

LocalFusion is designed as a **local single-user tool** (binds to `127.0.0.1` by default). Security measures in place:

- Provider keys are encrypted at rest with ChaCha20-Poly1305 (key derived from machine-id via HKDF-SHA256 with a random salt).
- Ingress keys and the admin token are stored only as SHA-256 hashes; plaintext ingress keys are returned only at creation time.
- The admin token is printed via `println!` only on first startup and is never written to logs.
- All SQL uses parameterized queries; no string concatenation.
- The inference endpoint requires key + ACL auth; every admin API endpoint requires admin token auth.
- Upstream error bodies are truncated and sanitized before being returned; the SSE output buffer is capped to prevent a misbehaving upstream from exhausting memory.

> If you expose LocalFusion on an address other than `127.0.0.1` (by changing `inference_bind` / `admin_bind`), you are responsible for evaluating your network-layer security — this tool has not been hardened for public multi-tenant deployments.

---

## License

This project is licensed under the [Apache License 2.0](LICENSE).
