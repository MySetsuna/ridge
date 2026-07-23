# CONTRACT — iteration 11

日期：2026-07-23
主题：M1 切片一——suspended panes 持久化与启动恢复（主线）+ P2 阶段 2 设计

## 双轨制与红线

用户轨四件不变。**红线（iteration 11 起录入纪律）**：无用户轨证据不启动扩协议面/新状态源/改 E2EE 的功能开发；M1 sidecar 为已定设计的本地镜像，不算新状态源。零 Remote 协议面变更。

## 自动轨目标

### G1[M1·主线] 切片一：suspended panes 落 sidecar + 启动恢复

- 落点按设计（`2026-07-23-workspace-memory-design.md` 选型 B）：`{app_data}/workspace-memory/{workspace_id}.json`，本切片仅 `runtime.suspendedPanes` + `updatedAt`；原子写（temp+rename）。
- 写方：`suspend/resume/clear_pane` 后 debounce 落盘（沿 `schedule_auto_save` 模式，经 `state.app_handle`）；读方：应用启动载入并重挂注册表。
- 生命周期：`close_workspace_core` 单点挂 sidecar 删除（best-effort）。
- 韧性（替代不可判定的时延停机线）：**IO 失败 fail-open**——暂停语义照常、log warn、不阻断关闭/启动；损坏 json 启动不 panic、重建空态。
- 验收：`cargo test -p ridge --lib` 与 `cargo test --workspace` exit 0，新测试覆盖：持久化→新注册表载入后 `is_suspended` 仍真；resume 后落盘态同步清除；损坏 json 载入不 panic 且为空态；close 清理文件。持久化模块 dir 注入可测（不依赖真实 app 目录）。
- 停机：若 `app_handle` 可用时机与启动载入顺序存在不可调和竞态 → 降级为「首次 suspend 时懒载」并记录。
- 减法：本切片不持久化 goal/constraints/decisions/tasks（后续切片）；不做 Remote 可见性变更（拓扑投影已自然反映）。

### G2[P2] 阶段 2 裁决通道设计文档（零代码）

- `docs/superpowers/specs/2026-07-23-hitl-resolution-v2-design.md`：nonce 防重放（挑战-响应 or 服务端一次性 id）、单次消费语义（PENDING 取出即毁 vs 标记）、过期与 fail-closed 交互（既有 120s 超时如何与远端裁决竞合）、多 controller 冲突（首达裁决生效？须审计双方）、审计记录规范（接 M1 `decisions` 切片二，只存风险分类+摘要）、传输面选型（`teammate` 能力下新方法 vs 0x12 CONTROL 通道，比较后择一）、桌面/远端一致性。
- 验收：文档提交；零代码 diff。
- 减法：排除审批委托/批量裁决等边缘逻辑，仅核心单条裁决流。

### G3[节律] 维护与闭环

- 全门禁维持绿（cargo/vitest/svelte-check）；闭环前刷审查导读（WORKFLOW 常规）。

## 自动验收

```powershell
cargo test --workspace
pnpm exec vitest run packages/remote/src/shared/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
node scripts/generate-review-pack.mjs
```

## 明确不做

- 不扩协议面；不实现 P2 阶段 2；不做 G1 阶段二 OS 冻结（Windows 竞态缺口裁决已存照，排期另议）；不并 M2（待 M1 切片一落地后下轮裁决）；本切片不持久化 decisions/goal 等其余字段。
