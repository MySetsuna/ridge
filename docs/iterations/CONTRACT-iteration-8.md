# CONTRACT — iteration 8

日期：2026-07-23
主题：P2 第一阶段——Remote HITL 待审批只读可见 + G1 设计文档 + A1/S1 支线

## 双轨制

自动轨（本合同验收对象）与用户轨（`docs/plans/user-verification-checklist.md` 四件，未消化）分列。功能集冻结：本轮唯一协议面新增限 P2 范畴（`teammate` 能力下新增只读方法），不加新 capability、不加裁决通道。

## 自动轨目标

### G1[P2·主线] `list_hitl_pending` 只读端到端

- 扩 `hitl.rs` PENDING 注册表值：存 sender 之外并存脱敏元数据 `{initiator, level, reason, created_at}`（同一注册表加宽，非新状态源；resolve/超时路径同步清理不变）。
- 新只读方法 `list_hitl_pending`（挂 `teammate` 能力下）：Rust `REMOTE_ALLOWLIST` + TS 镜像 + `REMOTE_CAPABILITY_METHODS.teammate` + LAN/cloud host 路由五处同步；`CLI_CAPABILITIES` 不含 teammate 故 rdg 自然 denied。
- **投影脱敏（安全关键）**：远端载荷仅 `{id, initiator, level, reason, createdAt}`——**绝不含 `action`**（命令全文可含密钥）。Rust 测试钉死投影无 `action` 字段与敏感子串；桌面事件载荷（含 action）不变。
- UI：Team 面板追加「待审批」只读区（列表 + 风险标签；无待审批零噪声）；不做任何裁决按钮。既有负断言维持：`resolve_hitl_request`/`set_hitl_enabled` 不可远达。
- 验收：`cargo test --workspace` exit 0（含新投影脱敏测试）；vitest 合同/桥/UI 测试绿；svelte-check 0 errors。
- 停机：若发现无法在 Rust 层把 `action` 排除出投影（结构我方全控，预期不触发）→ 停做并报告。

### G2[G1] Agent 暂停/恢复跨平台设计文档（零代码）

- `docs/superpowers/specs/2026-07-23-agent-suspend-resume-design.md`：可暂停边界（PTY/进程/agent 会话三层）、Windows 路线（无 SIGSTOP：job object 冻结 vs stdin 门控 vs PTY 暂停，选型+理由）、Unix SIGSTOP/SIGCONT、恢复语义与 HITL 交互、失败/超时 fail 方向。
- 验收：文档提交；本目标 git diff 仅含 docs（零代码/枚举变更）。
- 减法：不加 `Suspended/Resuming` 枚举变体（无实现的死状态，驳回 NotebookLM）。

### G3[A1] pane.rs 分类审计

- `src-tauri/src/commands/pane.rs`（及 ridge-core pane 对应面）只读 vs 写路径分类；rustc dead_code + 全仓 grep 双证；确凿死代码删（**可为零**，沿 iteration 7 Teammate 先例），存疑者只报告。
- 验收：审计结论入迭代报告；`cargo test --workspace` 维持 exit 0。
- 明确不做：写路径同源化（高风险，真机证据空窗期不动）。

### G4[S1] F3/F4 进程内计数 + F5 删除审计

- F3（controller TOFU 指纹变化 warn-only）与 F4（host 无身份密钥 → 0x01 帧）各加进程内计数器，沿 F1/F2 模式（无新持久面、无 telemetry.log——驳回 NotebookLM 持久日志）；测例钉死恰好 +1。
- F5：`keyBindingVerifier` 全仓消费审计（codegraph + grep）；确认生产零接线则删除钩子（矩阵 F5 行改「已退役（删除）」），有接线则只报告。
- 验收：vitest 计数/删除后全伞绿；矩阵文档同步更新。
- 停机：F5 若删导致任何测试红且非纯钩子引用 → 回滚删除、降级为报告。

## 自动验收

```powershell
cargo test --workspace
pnpm exec vitest run packages/remote/src/shared/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
node scripts/run-weaknet-lab.mjs   # 回归护栏
```

## 明确不做（冻结清单）

- 不做 HITL 裁决通道（nonce/单次消费/过期/多 controller 语义，第二阶段）。
- 不做 M1/M2/H1/C1/E1/E2；不加新 capability；不做写路径同源化。
- 不引入任何持久遥测面或 CI 假设（本仓无 CI）。
