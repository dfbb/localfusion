# LocalFusion 构建
1. 前端：`cd web && pnpm install && pnpm build`（产物到 web/dist）
2. 后端：`cargo build --release`（rust-embed 嵌入 web/dist）
3. 运行：`./target/release/localfusion --db ./localfusion.db`
   首次启动控制台打印 admin token（仅一次），用它登录管理端口 127.0.0.1:8788。
CI：两步串联（先 pnpm build，再 cargo build）。dist 缺失时后端仍可编译（前端显示占位页）。
