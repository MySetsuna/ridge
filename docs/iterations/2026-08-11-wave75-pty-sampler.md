# Wave 75：PTY 生产采样器闭环（2026-08-11）

## 结论

本轮已把 PTY fallback 从“只有安全边界”推进为“有真实桌面采样器”。RidgePane 每秒采样一次，并在 PTY 输出、用户输入、IME 事件、粘贴事件发生时立即采样；采样结果带 `stateRevision`、`inputEpoch`，经 Tauri 命令写入 Agent Hub。

采样器只接受 Kernel roster 中按 `workspaceId + paneId` 找到的在线 Agent 身份，且必须带非零 `generation` 与非空 `lease`。普通 shell、UI 手工标记但没有 Kernel 身份的 pane 均返回 `null`、不发布证明；故仍 fail-closed。

## 本轮改动

- `src/lib/terminal/ptyRuntimeSnapshot.ts`：集中生成五条件快照。
- `src/lib/terminal/ptyRuntimeSnapshot.test.ts`：覆盖安全路径与八类不安全分支，并校验计数器归一化。
- `src/lib/components/RidgePane.svelte`：接入生产采样器、HITL 刷新、输入 epoch、采样生命周期与 PTY 销毁清理。
- `src-tauri/src/teammate/mcp.rs`：从 Kernel roster 读取 pane 身份；按 live pane + Kernel `generation/lease` 双重围栏后发布。
- `src-tauri/src/commands/teammate.rs`、`src-tauri/src/lib.rs`：注册 `get_pty_runtime_identity`。
- `src-tauri/src/commands/pane.rs`、`src-tauri/src/commands/terminal.rs`：释放或销毁 PTY 时清除旧证明。

关键修正：Kernel Agent 的稳定 ID 为 `kernel:<paneId>`，不能依赖 UI `teammate_agent_pane_map` 的展示名称；身份选择现以 Kernel 的 `workspaceId + paneId + online + generation + lease` 为准，UI 映射不再冒充权限来源。

## 验证证据

- `pnpm check`：0 errors / 0 warnings。
- `pnpm test`：215 files，1975 passed，1 skipped。
- `cargo fmt --all -- --check`：通过。
- `cargo test -p ridge-mcp --lib --quiet`：90/90。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib teammate::mcp::tests --quiet`：6/6。
- `node scripts/cdp-lan-probe.mjs`：hello、pane UUID、scrollback/live frame、echo、pong 全通过。
- `node scripts/cdp-dpr-e2e.mjs`：DPR `1.5`，Canvas/backing canvas 均通过。
- `node scripts/cdp-remote-mobile-agents.mjs`：真实 `/verify`、移动 SPA、数据面与能力降级通过。
- `node scripts/cdp-pty-parsers.mjs`：在带固定远程 CA 的冷启动复核中，既有一次 ConPTY settle 超时；历史稳定运行与本轮第二次复核分别出现 UTF-8、OSC title/cwd 单项收齐，说明 harness 仍存在冷启动/重复 pane 非幂等噪声，不能把该脚本本轮结果记作稳定绿门。

本轮还收紧了 Remote pane feed 的预算边界：`PaneFeedScheduler` 与 `TerminalManager.flushPaneFeed` 均拒绝 `Infinity` 穿透，并在清空/销毁时取消已排队 frame；对应 focused tests 为 `29/29`。

SonarQube 本轮尝试以临时 token 扫描产品源，scanner 运行约 10 分钟无结果，已停止精确扫描进程并撤销临时 token；未将其记为本轮新扫描成功。服务器仍为 `UP`，当前 API 可见的最后成功项目指标为 coverage `80.4%`、line `86.7%`、branch `71.5%`、Quality Gate `OK`、`new_violations=0`，该指标沿用既有分析记录，非本轮扫描产物。

## 仍需现场验收

- 真实第三方 Agent Runtime/A2A 私有协议兼容性。
- 真实公网/TURN、双真实窗口、实体手机 PWA/IME/后台恢复、物理 DPR/像素矩阵。
- 新采样器在真实 Agent CLI 的五条件动态切换与 HITL 审批现场证据；本地普通 shell 与 fake-agent harness 不代替该证据。

发布、push、tag、Release 未执行；Sonar 凭据未写入仓库。
