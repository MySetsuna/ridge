# NLM 下一轮需求审计（2026-08-11）

## 结论

NLM 笔记中可由本仓库完成的核心需求，当前源码已基本落地；NLM 最近回答仍引用旧快照，误报了 Message Hub、Grok adapter、kernel host 与 MCP sidecar 缺失。当前不能宣称“全部需求完成”，因为公网、真机、原生 PowerShell 像素对照、双窗口/双 Host 物理验收及 Sonar Quality Gate 尚无可复核证据。

## 当前源码复核

| 需求 | 当前事实 | 证据 |
| --- | --- | --- |
| `REQ-RIDGE-MCP-AS-KERNEL-API-01` | `McpSessionState::with_sqlite`、入队幂等、收件箱/回执/取消、Runtime/A2A/MCP pull/受护 PTY fallback 已存在；MCP bridge 默认发现 kernel，kernel 不在时 fail-closed。 | `packages/ridge-mcp/src/server.rs`、`packages/ridge-mcp-bridge/src/lib.rs`；ridge-mcp 单测 90/90。 |
| `REQ-RIDGE-KERNEL-HOST-01` | kernel detect-or-spawn、跨进程 boot slot、单实例、death watcher、shutdown 与 Y/N 确认入口已存在。 | `src-tauri/src/kernel_lifecycle.rs`、`packages/ridge-cli/src/kernel_ctl.rs`；kernel lifecycle E2E 3/3，Tauri lib 281/281。 |
| `REQ-RIDGE-KERNEL-DOMAIN-01` | kernel 提供 Agent、Remote host、workspace 等领域入口；shell 通过 kernel client 投影。 | `packages/ridge-kernel/src/kernel_mcp.rs`、`packages/ridge-kernel/src/domain.rs`、`src-tauri/src/hosts/mod.rs`。 |
| `REQ-AGENT-HISTORY-SOURCE-02` | Claude/Codex 按 `(agent, session_id)` 聚合；Grok 适配真实目录 `~/.grok/sessions/<cwd>/<session>/summary.json + chat_history.jsonl`；未知格式不猜，resume 无证据则禁用。 | `src-tauri/src/commands/project.rs`；Claude/Codex/Grok fixture、真实 home 扫描测试已在 Tauri 281 项中通过。 |
| `REQ-RIDGE-MCP-INSTALLER-01` | sidecar 构建、目标 triple、版本嵌入、`--check --require-built`、Tauri `externalBin`、release bundle 检查已存在。 | `scripts/build-ridge-mcp-sidecar.mjs`、`packages/ridge-mcp-bridge/`、`src-tauri/tauri.conf.json`、`.github/workflows/release.yml`。 |
| `REQ-AUTO-CONTRAST-RESEARCH-01` | 已按“仅研究”完成静态 token/WCAG 2.2 阻断、APCA/WCAG 3 影子报告、forced-colors fixture 与减法方案；未引入全局运行时逐像素判色。 | `docs/research/auto-contrast-2026-07-29.md`。 |
| `REQ-RDG-REMOTE-CONNECT-01` | LAN 桌面/移动受控 E2E 已真接通；公网与真机仍待外部环境。 | `.iteration/artifacts/rdg-remote-e2e/`；`rdg-remote-e2e ALL PASS`。 |

## 本轮验证

- `pnpm check`：0 errors / 0 warnings。
- `pnpm test`：216 files，1996 passed，5 skipped（含按 `RIDGE_WEB_REMOTE` 构建标志选择的剪贴板测试）。
- `pnpm test:coverage:sonar`：216 files，1996 passed，5 skipped；全量报告 lines 70.23%、branches 60.44%。按 Sonar 排除规则过滤 `src/` 与 `packages/` 后，源码 lines 87.24%、branches 71.82%。
- 本轮新增 flag 字体 DOM/cache/探测异常路径、远端剪贴板构建路径、remote pane 缓冲/激活/删除路径单测；桌面构建针对测试通过，`RIDGE_WEB_REMOTE=1` 构建路径测试通过。
- 验证 `scripts/sync-generated-csp.mjs` 对标准 `Content-Security-Policy" content="..."` meta 的处理；CSP 回归 4/4 通过。
- `cargo test -p ridge-cli --test kernel_lifecycle_e2e --quiet`：3 passed，exit 0。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --quiet`：281 passed，exit 0。
- 既有 Rust crate 单测、clippy、fmt、CDP smoke/PTY/DPR/LAN probe、Remote LAN desktop/mobile、mobile keyboard 均已通过；详见 `docs/iterations/2026-08-11-wave88-remote-cdp-csp.md`。
- Grok 本机形态已抽样确认：assistant 记录为 `{type, content, tool_calls, model_id, ...}`，`content` 为字符串；当前解析器覆盖该形态。

## 尚未闭环项

以下不是当前源码缺失，或不能靠继续猜测代码闭环：

1. 公网 × 桌面、 公网 × 手机的真实 relay/WebRTC/TURN/E2EE 接通与断连重连。
2. iOS/Android 真机键盘、visualViewport、后台 15 分钟保活与 PWA 自愈。
3. 真实 Windows PowerShell/PTY 在 DPR 1.25/1.5/2 的像素对照矩阵。
4. 双窗口争用同一 Remote workspace、双 Host 与焦点切换的物理证据。
5. 受影响手机上的 clean-profile/无扩展 A/B，以归因 `runtime.lastError` 是否第三方注入。
6. Windows 跨卷、权限拒绝、部分失败的真实 NTFS Explorer 矩阵。
7. SonarQube Quality Gate：本机服务 `http://127.0.0.1:9000` 可用，但当前未获得可复核的本次 CE/Gate 终态；服务密码不写入仓库，扫描应使用临时 token。

## 授权边界

本轮未发布、未 push、未创建 release。公网、真机与最终 Sonar Gate 需要外部环境或凭证；不以 fixture、服务 UP 或旧 NLM 结论替代现场证据。
