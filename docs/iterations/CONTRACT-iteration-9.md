# CONTRACT — iteration 9

日期：2026-07-23
主题：G1 阶段一软暂停/恢复（纯本机）+ 审查闭环与簿记清偿（不扩 Remote 协议面）

## 双轨制

自动轨如下；用户轨四件不变（`docs/plans/user-verification-checklist.md`）。**本轮零 Remote 协议面变更**：不动 `REMOTE_ALLOWLIST`/能力表/矩阵 methods。

## 自动轨目标

### G1[G1·主线] Agent 软暂停/恢复阶段一（设计文档阶段一的实现）

- 门控点 = **agent 写路径收口**（teammate send-keys 网关，与 HITL `request_approval` 同一咽喉）：suspended agent 的 send-keys 被拒并返回明确「agent suspended」错误；**桌面人类输入不受限**（接管语义）。
- 运行态侧表（类比 `teammate_pane_states`，进程级，不改 `Teammate` 结构）：`suspend(agent_id)` / `resume(agent_id)` / 查询；幂等（重复 suspend no-op 成功）；pane 关闭 / agent 释放时清理（沿 `release_teammate_agent`/`remove_by_pane` 路径）。
- 状态枚举与实现同轮出现（设计 §3 承诺）；桌面 Agent Center UI 暂停/恢复按钮 + 状态标识；`get_teammate_topology` 投影 status 反映 Suspended（只读，Remote 自然可见，无新方法）。
- 验收：`cargo test --workspace` exit 0，含新单测（门控拒绝、恢复放行、幂等、清理、suspended 状态入拓扑投影且无新敏感字段）；svelte-check 0 errors。
- 停机：若 agent 写路径入口不唯一（存在绕过 send-keys 网关的第二写径）→ 本轮先收敛写入口，暂停功能顺延。
- 减法：不做 OS 级冻结（阶段二）、不做 Remote 控制按钮、不改协议。

### G2[C1] rdg 语义缺口报告（半自动）

- `scripts/rdg-gap-report.mjs`：读 `docs/capability-matrix.json`，派生 rdgHost denied/not-applicable 能力与方法清单 + 与桌面语义差值，写 `docs/audits/rdg-gap-report.md`；脚本幂等可重跑。
- 验收：脚本 exit 0；报告提交；不写任何 rdg 功能代码。
- 停机：矩阵出现无法静态派生的动态能力 → 如实记录该项为「需人工判定」。

### G3[审查辅助] 分支审查导读包

- `scripts/generate-review-pack.mjs`：`git log origin/main..HEAD` 按 conventional type(scope) 分组，每提交列文件数/±行/触及面标注（协议面 = capability.rs / remoteAllowlist / capabilityContract / matrix 路径探测；安全面 = hitl / e2ee / totp / trust 路径），输出 `docs/review/branch-review-guide.md`。
- 验收：脚本 exit 0；导读覆盖全部领先提交（数量与 `git rev-list --count` 一致，脚本内自校验）。
- 减法：只做导读不做合并自动化。

### G4[E1/E2] 差距簿记校正（零代码）

- E1：PROJECT-STATE §3.1 与 §6 措辞校正——WebGPU 为**生产默认特性**（运行时探测 + Canvas2D 回退，2026-05-05 用户反馈钦定）；差距重定义为「真机 GPU 收益测量」（用户轨）。**驳回并留档 NotebookLM 的删除建议**（guidance-8 已记）。
- E2：状态改「已关闭——待真实多 Agent 瓶颈证据重开」。
- 验收：PROJECT-STATE 更新提交；无代码 diff。

### G5[A1] workspace 写路径审计文档（零代码）

- `docs/audits/workspace-write-paths.md`：列 Tauri 命令层与 ridge-core 在工作区增删改（create/close/rename/reorder/save/delete）上的调用重叠点与同源化候选顺序；无逻辑变更。
- 验收：文档提交；`cargo test --workspace` 维持 exit 0（未动代码自然成立）。

## 自动验收

```powershell
cargo test --workspace
pnpm exec vitest run packages/remote/src/shared/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
node scripts/rdg-gap-report.mjs
node scripts/generate-review-pack.mjs
```

## 明确不做（冻结清单）

- 不扩任何 Remote 协议面（allowlist/能力/矩阵 methods 零变更）。
- 不删 WebGPU（生产默认路径）；不定义无消费者的 M1 结构体；不做 OS 级进程冻结；不做 P2 阶段 2。
- 不引入持久遥测面；本仓无 CI，不写 CI 假设。
