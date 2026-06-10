# Dev 全链路 TLS（wss）设计

- 日期：2026-06-10
- 状态：已评审，待实施
- 涉及仓库：`wind`（客户端）、`ridge-cloud`（云端，`C:\code\ridge-cloud` 同级仓库）

## 背景

dev 模式下点击登录打开浏览器授权会报「网络错误」。根因排查（见会话记录）确认 dev 登录链路存在三重错配：服务未起在 `localhost:5001`、cloud 与客户端 http/https 协议不一致、CORS 拒绝 dev webview 源。其中「连接被拒」与「CORS」已先行修复（CORS 放行分支已落在 `ridge-cloud/src/router.rs::is_allowed_origin`）。

本设计解决剩下的**协议方向**问题：开发者希望 dev 也跑 **TLS（https + wss）**，而非明文，以完整复刻生产形态（生产恒为 https/wss），并打通远控信令子域 `wss://{device}-{username}.localhost:5001`。

难点在于 dev 自签证书默认不被 WebView2/Chromium 信任，且现有证书 SAN 不含 `*.localhost` 通配。

## 目标 / 非目标

**目标**
- dev 默认全链路 TLS：cloud 起 `https://localhost:5001` + `wss`，客户端对回环 cloud 也走 `https`/`wss`。
- 远控信令子域 `wss://{device}-{username}.localhost:5001/ws` 在 dev 下证书匹配、可连。
- WebView2/Chromium 信任 dev 证书（无 `ERR_CERT_*`）。

**非目标**
- 不让 vite dev server（webview 页面源）上 TLS——仍是 `http://127.0.0.1:5173`。http 页面访问 https/wss 后端是允许的（`127.0.0.1` 属 secure context，不构成 mixed-content 降级）。
- 不改动生产 TLS / CORS 行为。

## 关键设计决策

1. **证书工具：mkcert**。用 mkcert 建本地受信 CA 并签 `*.localhost` 通配证书。`mkcert -install` 把 CA 装进 Windows 受信根，WebView2/Chromium 走系统证书库自动信任。
2. **dev 默认 wss**（非 opt-in）。回环 cloud 默认走 TLS；保留一个 documented 逃生开关 `RIDGE_CLOUD_DEV_PLAINTEXT=1`（客户端）/ `RIDGE_CLOUD_DEV_HTTPS=0`（cloud）以备 mkcert 故障时临时回明文。
3. **cloud 未装 mkcert 时保留 rcgen 自签回退**，CI / 未装环境启动不崩（仅证书不受信）。
4. **证书存放** `%LOCALAPPDATA%\ridge\cloud-tls\`（复用现有 `tls.rs::tls_dir`，跨 repo、天然不入 git）。

> 一致性说明：本设计的「dev 默认 wss」**回调**了先前为「明文对齐」对 `ridge-cloud/scripts/dev.sh` 所做的「默认 http」改动（dev.sh 改回默认 https/wss）。但 `router.rs` 的 CORS 回环 http 源放行分支**保留且必需**——dev webview 页面源仍是 `http://127.0.0.1:5173`，对 https cloud 发请求时 CORS 仍需放行该源。

## 架构与组件

### A. 证书与信任（mkcert，一次性）
- `scoop install mkcert` → `mkcert -install`。
- `mkcert -cert-file cert.pem -key-file key.pem localhost "*.localhost" 127.0.0.1 ::1` → 输出到 `%LOCALAPPDATA%\ridge\cloud-tls\`。
- 新增 `ridge-cloud/scripts/setup-dev-tls.mjs`（幂等）：检测 mkcert（缺失则提示 `scoop install mkcert` 并退出非零）→ `mkcert -install` → 生成上述通配证书到约定路径。被 `dev.sh` 在启动 cloud 前调用。

### B. cloud 侧（ridge-cloud）
- `src/tls.rs`：
  - 新增 **BYO 证书优先**路径——若 env `RIDGE_CLOUD_TLS_CERT`/`RIDGE_CLOUD_TLS_KEY` 指定（或约定路径存在 mkcert 证书）则直接 `RustlsConfig::from_pem` 加载；否则回退现有 rcgen 自签逻辑。
  - mkcert 证书 SAN 含 `*.localhost`，子域信令 TLS 由此满足。
- `scripts/dev.sh`：
  - 改为**默认 https/wss**：先执行 `node scripts/setup-dev-tls.mjs`，再 `export RIDGE_CLOUD_DEV_HTTPS=1` + 证书路径 env；banner 显示 `https://localhost:5001`、`wss://…`。
  - 保留 `RIDGE_CLOUD_DEV_HTTPS=0` 逃生（回明文）。
- `src/router.rs`：CORS **不改**（既有回环 http 源放行分支继续生效）。
- `src/main.rs`：dev TLS 分支已存在，无需结构改动（仅 env 默认值经 dev.sh 调整）。

### C. 客户端（wind）
- `src/lib/remote/cloud/apiClient.ts`：
  - `cloudHttpScheme`/`cloudWsScheme`：回环 cloud 默认返回 `https`/`wss`（dev 默认 TLS）。
  - 逃生开关：构建期 define `RIDGE_CLOUD_DEV_PLAINTEXT`（经 `vite.config.js` 注入，类比 `RIDGE_CLOUD_BASE_DOMAIN`），为真时回环降级 `http`/`ws`。默认假。
  - `isInsecureCloudDomain` 保留（仍只判「是否回环」的纯函数，供逃生开关判定回环用）。
- `vite.config.js`：新增 `import.meta.env.RIDGE_CLOUD_DEV_PLAINTEXT` define。
- webview/devUrl 维持 `http://127.0.0.1:5173`（不改）。

### D. 数据流（验证路径）
1. `cd ridge-cloud && ./scripts/dev.sh` → setup-dev-tls 生成/复用 mkcert 证书 → cloud 起 `https`/`wss`@5001。
2. `pnpm tauri dev` → webview 加载 `http://127.0.0.1:5173`。
3. 点登录 → `fetch https://localhost:5001/api/v1/auth/request`（证书受信 ✓、CORS 放行 ✓）→ 浏览器授权 → 轮询 approved。
4. 远控 → 信令 `wss://{device}-{username}.localhost:5001/ws`（`*.localhost` 通配证书 ✓，Chromium 自动解析 `127.0.0.1`）。

## 测试策略

- **cloud（Rust 单测，TDD）**：`tls.rs` 证书来源选择——
  - 给定 `RIDGE_CLOUD_TLS_CERT/KEY` 指向有效 PEM → 走 BYO 加载；
  - 未指定 → 回退 rcgen 自签。
  - （`RustlsConfig` 加载本身依赖文件 IO，单测聚焦「来源选择」纯逻辑，必要时抽出可测函数。）
- **客户端（vitest）**：更新 `apiClient.test.ts`——回环 `cloudHttpScheme`/`cloudWsScheme` 默认期望 `https`/`wss`；新增 `RIDGE_CLOUD_DEV_PLAINTEXT` 为真时回 `http`/`ws` 的用例；`isInsecureCloudDomain` 既有断言保留。
- **手动 e2e**：起 cloud + `tauri dev`，完成登录 + 远控连接，确认 DevTools 无 `ERR_CERT_*` / CORS 报错。

## 风险与回退

- **mkcert 未装 / `-install` 需管理员**：`setup-dev-tls.mjs` 检测缺失即明确提示安装命令；`mkcert -install` 首次可能弹 UAC（一次性）。
- **WebView2 证书缓存**：换证书后可能需重启 webview。
- **逃生通道**：客户端 `RIDGE_CLOUD_DEV_PLAINTEXT=1` + cloud `RIDGE_CLOUD_DEV_HTTPS=0` 成对回明文，且 CORS 明文放行分支仍在，可随时退回上一版明文链路调试。

## 文件改动清单

**ridge-cloud**
- `scripts/setup-dev-tls.mjs`（新增）
- `scripts/dev.sh`（默认 https/wss + 调 setup-dev-tls）
- `src/tls.rs`（BYO 证书优先 + 单测）

**wind**
- `src/lib/remote/cloud/apiClient.ts`（scheme 默认 TLS + 逃生开关）
- `src/lib/remote/cloud/apiClient.test.ts`（更新断言）
- `vite.config.js`（注入 `RIDGE_CLOUD_DEV_PLAINTEXT` define）

## 验收标准

- `dev.sh` 起 cloud 后，浏览器访问 `https://localhost:5001` 与 `https://x.localhost:5001` 均证书受信。
- `pnpm tauri dev` 下点登录走 `https`，授权成功，无网络错误。
- 远控信令 `wss://{device}-{username}.localhost:5001` 连接成功。
- cloud `cargo test` 与 wind `apiClient.test.ts` 全绿。
