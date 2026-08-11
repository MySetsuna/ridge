# NLM 下一轮验证增量（2026-08-11）

本文件覆盖主审计中较早的验证数字；需求结论仍以 `docs/iterations/2026-08-11-nlm-next-wave-audit.md` 为准。

## 本轮已落地

- 修复 Explorer/上下文动作的 workspace 相对路径漂移：不再依赖首个 pane/column cwd，改取同 workspace 全部 cwd 的公共祖先；跨盘或无公共祖先则安全回退当前 cwd。
- 新增 `commonPathAncestor` 分支测试：Windows drive、UNC、POSIX、相对路径、无公共祖先、跨根、单路径、空输入；`pnpm exec vitest run src/lib/utils/path.test.ts` 为 4 passed。

## 最新质量证据

- `pnpm check`：0 errors / 0 warnings。
- `pnpm test`：216 files，1997 passed，5 skipped。
- `pnpm test:coverage:sonar`：216 files，1997 passed，5 skipped；全量 lines 70.31%、branches 60.54%。
- `pnpm build:remote:mobile`：3936 modules，PWA precache 38 entries，PASS。
- `pnpm build:remote:desktop`：4231 modules，PASS，耗时 3m30s；仍有 Vite dynamic-import/chunk-size warning，但无构建失败。
- 本轮重跑 `pnpm cdp:smoke`、`pnpm cdp:pty`、`cdp-dpr-e2e`、LAN desktop/mobile E2E、mobile keyboard E2E，均 PASS。mobile keyboard 仍是 Chromium 模拟，不是真机证据。

## 仍未闭环

1. 公网 relay/WebRTC/TURN/E2EE 的桌面、手机真实接通与断连重连。
2. iOS/Android 真机键盘、visualViewport、后台 15 分钟保活与 PWA 自愈。
3. Windows PowerShell/PTY 在 DPR 1.25/1.5/2 的真实像素矩阵。
4. 双窗口/双 Host/焦点切换、多 Host 物理验收。
5. 真实 NTFS 跨卷、权限拒绝、部分失败矩阵。
6. SonarQube Quality Gate：服务 `http://127.0.0.1:9000` UP，但 scanner 在 `/api/v2/analysis/version` 收到 HTTP 401；未取得 CE/Gate，未写入或猜测服务密码。
7. `pnpm e2e:runtime-attribution` 仅为 clean-profile，`attributionComplete=false`。

本轮未 push、未发布、未创建 release；上述外部证据需真实环境或授权凭证，不能用 fixture/模拟器替代。
