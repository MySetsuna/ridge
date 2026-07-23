# CONTRACT — iteration 6

日期：2026-07-23
主题：P1 Remote Agent 控制台 MVP（capability `teammate`）+ S1 遥测计数 + A1 workspace 只读同源化

## 目标

### G1[主线·P1] Remote Agent roster 只读可见 + 切 Pane

- 新 capability **`teammate`**，方法面仅 `get_teammate_topology`（只读，轮询取数，不建订阅流）。
- 宣告与放行（A2 纪律：先声明后实现，六处同步）：
  1. `ridge-core::REMOTE_ALLOWLIST` + `MUTATING_METHODS` 不收录（只读）；
  2. `capabilityContract.ts::REMOTE_CAPABILITY_METHODS.teammate = ['get_teammate_topology']`；
  3. `rpcClient.ts::CLIENT_CAPABILITIES` + 桌面 LAN `HOST_CAPABILITIES` + `cloudHostBridge.ts::HOST_CAPABILITIES` 增列；`ridge-cli::CLI_CAPABILITIES` **不**增列（无 roster 数据源）；
  4. `docs/capability-matrix.json`：desktop/lan/cloudDesktop/cloudMobile=supported，rdgHost=denied，rdgController=not-applicable；一致性测试随动。
- 路由：LAN 桌面 host 与 cloud host（invoke 透传）都能服务 `get_teammate_topology`（沿既有非 core 方法的 host 路由样式）；TS `remoteAllowlist` 镜像同步。
- UI（共享 controller，桌面+移动同码）：能力协商后显示 roster 视图——成员名、角色、Working/Idle 状态、所在 pane；点击成员切到对应 pane（复用现有 pane 切换路径）；无团队时零噪声（不渲染空面板）。
- 安全断言测试：对回退路径与 typed-profiles 路径的序列化拓扑，断言不含 `token`/`endpoint`/`env` 字样字段；`resolve_hitl_request`/`set_hitl_enabled` 保持不在 allowlist（负断言）。

### G2[S1] F1/F2 回落计数实施（按设计文档，不落新文件）

- `cloudHostBridge`：trust-proof 处理处计数 `transcript_present: true/false`（F1）。
- provider（controller/host）：绑定判定终态计数 `enforced/relay-trust`（F2）。
- 形态：进程内计数器 + 现有诊断日志一行；单测断言对应路径恰好 +1。不新增持久化文件、不上报。

### G3[A1] workspace 只读三件套同源化

- `commands/workspace.rs` 的 `list_workspaces` / `get_active_workspace_id` /（如可行）`get_workspace_snapshot` 改为委托 ridge-core 同名 handler（对照 git.rs 薄委托样板）。
- 每件独立提交；桌面/Remote 行为回归以既有测试 + `cargo check -p ridge` + svelte-check 判定。
- 停机条件：core handler 与桌面命令的返回形状有不兼容差异时，先记录差异不强改前端。

## 自动验收

```powershell
pnpm exec vitest run packages/remote/src/shared/transport/ packages/remote/src/shared/cloud/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
cargo test --workspace --exclude ridge
cargo test -p ridge --bins
cargo check -p ridge
```

## 停机条件

- 拓扑序列化在任一路径含敏感字段：停止暴露，先修投影。
- 新能力宣告与矩阵/一致性测试冲突：以 canonical 声明为准修矩阵，不 hack 测试。
- G3 形状不兼容：记录并顺延，不为凑同源改协议形状。

## 明确不做

- 不做订阅流/推送、不做 HITL 远端裁决（P2）、不做 kill/暂停 Agent（G1 差距）、不做 Agent 配置写路径。
- 不动 WebGPU 构建面（E1 未证伪）；不挂 T3 进 CI（用户决策）。
- 真机 smoke、生产 status 实跑、两分支合并部署仍为用户必办件。
