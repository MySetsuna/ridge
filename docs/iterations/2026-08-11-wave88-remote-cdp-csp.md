# Wave 88：Remote CDP / CSP / DPR 收口

## 结论

- `dev:cdp` 已真实启动；动态 CDP 端口可发现。
- Remote LAN 桌面/移动浏览器矩阵通过；PTY、LAN 协议、DPR 画布探针通过。
- 远端静态 SPA 曾因 SvelteKit 生成 inline bootstrap script 的 CSP hash 漏配而空白，已改为构建后按最终 HTML 自动补 hash；不放开 `unsafe-inline`。

## 落地项

1. `scripts/sync-generated-csp.mjs`：为最终构建 HTML 的 inline script/style 计算并补入既有 CSP 指令。
2. `package.json` 与 `scripts/build-remote-desktop.mjs`：桌面构建、Remote 构建后自动同步 CSP。
3. `scripts/cdp-dpr-e2e.mjs`：target 等待上限与 app-ready 上限统一，避免冷启动约 30 秒时误报无页面。
4. `scripts/tauri-dev-cdp.mjs`：先构建桌面宿主，最后构建 `rdg`，再复制并执行陈旧产物闸门。

## 验证

- `pnpm check`：0 errors / 0 warnings。
- `pnpm test`：216 files，1986 passed，1 skipped。
- `cargo fmt --all -- --check`：通过。
- `cargo clippy -p ridge-core -p ridge-kernel -p ridge-mcp -p ridge-remote -p ridge-tmux --all-targets -- -D warnings`：通过。
- Rust：core 344、kernel 50、MCP 90、remote 28、tmux 11、ridge-term 既有回归均通过。
- CDP：`pnpm cdp:smoke`、DPR、`pnpm cdp:pty`、LAN protocol probe 均通过。
- Remote：`pnpm e2e:rdg-lan -- --skip-build --port 9528` 桌面/移动均 `PASS`；`pnpm e2e:rdg-mobile-keyboard` `ok=true`，`browserErrors=[]`。
- CodeGraph：sync 后复查 Canvas2D DPR 调用链、CDP target 等待链与构建入口。

## 边界

- LAN E2E 使用本机明确 CA：`%LOCALAPPDATA%\ridge\remote-tls\ca.pem`；未关闭 TLS 校验。
- Job 受限环境运行 PTY/CDP 时，需由测试启动前显式设置 `RIDGE_CDP_ALLOW_NON_BREAKAWAY=1`；生产默认仍 fail-closed。
- Sonar 扫描继续由本地实例运行；扫描过程中的既有 ts-rs warning 仅为生成属性解析警告，不等同业务失败，最终 Quality Gate 以服务器结果为准。
- 真实手机、公共 WebRTC/TURN、云端凭据与发布仍属外部门禁，未冒充本地通过。
