# Wave 85：Remote 背压与实时尾帧质量闭环（2026-08-11）

## 本轮落地

- `e32c1afc` 修复 `TerminalManager._feedNow` 的 deferred remainder 回填绕过
  `MAX_FEED_DEFERRED_BYTES` 问题；新增 FIFO 头部回填与溢出计数/重同步标记。
- `71dbea00` 使未有真实 resume 契约的 Gemini、Cursor Agent、Aider 内置 profile
  对恢复参数 fail-closed；识别仍可用，用户显式 override 不受影响。
- `48a06781` 删除已被 `rdg login` 取代且无调用方的旧 `device_flow` 模块，修正
  CLI 顶层用法；`4b50e7ca` 删除未使用 TUI workspace helper，并保留测试后端
  的 session drop 语义。
- `92c5332f` 更新 `REQ-MOBILE-REMOTE-LIVE-TAIL-01` 当前证据。

## 验证

| 检查 | 结果 |
|---|---:|
| Remote manager/feed policy 定向 Vitest | 2 files / 25 passed |
| Cloud live-tail 回归 | 52 passed |
| `cargo test -p ridge-kernel agent_profiles --lib --quiet` | 2 passed |
| `cargo test -p ridge-cli --bin rdg --quiet` | 155 passed |
| Kernel/MCP/lifecycle architecture regression | 50 / 90 / 3 passed |
| `cargo fmt --all -- --check` | passed |
| `pnpm check` | 0 errors / 0 warnings |
| `pnpm test:coverage:sonar` | 215 files / 1984 passed / 1 skipped / exit 0 |

本地 LCOV：statements `66.05%`、branches `60.22%`、functions `68.28%`、lines
`70.13%`。该报告包含本地配置范围，不能替代 Sonar 项目指标。

## 尚未闭合的外部门禁

Sonar 服务状态为 `UP`，但项目 Quality Gate API 仍返回 `401`；没有有效 scanner
认证，不能刷新页面中的旧 Gate 结果。物理移动/PWA、公网 Remote、第三方
Runtime/A2A、WebView2/DPR 与 PTY 五条件现场证据继续保持 ACTIVE。未写入凭据，未
push/tag/release。
