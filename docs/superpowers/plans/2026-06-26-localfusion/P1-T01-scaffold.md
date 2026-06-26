# P1-T01 工程脚手架

**阶段:** 1 基础层 · **前置:** 无 · 见全局约束: `00-index.md`

**Goal:** 建 Cargo 工程骨架与模块声明。

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs`
- Create 占位: `src/error.rs`, `src/crypto.rs`, `src/unified.rs`, `src/db/mod.rs`

**Produces:** crate `localfusion`，模块 `pub mod error; pub mod crypto; pub mod unified; pub mod db;`；`main()` 打印版本。

- [ ] **Step 1: 写 Cargo.toml**

```toml
[package]
name = "localfusion"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "localfusion"
path = "src/main.rs"

[lib]
name = "localfusion"
path = "src/lib.rs"

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "macros"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tracing = "0.1"
chacha20poly1305 = "0.10"
hkdf = "0.12"
sha2 = "0.10"
machine-uid = "0.5"
base64 = "0.22"
rand = "0.8"

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "test-util"] }
tempfile = "3"
```

- [ ] **Step 2: 写 src/lib.rs**

```rust
pub mod crypto;
pub mod db;
pub mod error;
pub mod unified;
```

- [ ] **Step 3: 写 src/main.rs**

```rust
fn main() {
    println!("localfusion {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 4: 创建占位模块文件**

`src/error.rs`、`src/crypto.rs`、`src/unified.rs` 内容 `// filled in later task`；`src/db/mod.rs` 内容 `// filled in later task`。

- [ ] **Step 5: 验证构建**

Run: `cargo build`
Expected: 编译成功（unused 警告可忽略）。

- [ ] **Step 6: 提交**

```bash
git add Cargo.toml Cargo.lock src/
git commit -m "chore: 工程脚手架与模块骨架"
```
