# NotebookLM guidance — iteration 2

生成时间：2026-07-21 12:39 +08:00  
Notebook：`Ridge 项目现状、愿景与规划基线（2026-07-21）`  
Conversation：`a47d3199-c1f9-47f1-927c-ff2c4875b77d`

## NotebookLM 原始指导

作为 Ridge 项目 Iteration 3 的规划 Maker，我基于《Iteration 2 报告》揭示的工程阻塞、GAP_PORTFOLIO 中的长期差距以及产品愿景，为您制定以下规划。

### 一、风险判定与前置关系分析

1. **最高风险与前置条件：`pnpm check` 持续超时（T1 变体）**
   - 判定：这是当前最紧急的阻断风险。
   - 依据：Iteration 2 报告显示 `pnpm check` 在 300 秒内零诊断输出并被强制杀死。如果静态检查器不可用，CI 无法证明新功能的类型安全，任何“完成”都是不可信的。它是后续所有开发的工程护栏。
2. **核心质量风险：弱网/长时 Fault-Injection（R1）**
   - 判定：这是愿景实现的核心挑战。
   - 依据：代码中已有重连、分片和背压机制，但缺当前指标。相比真机 Smoke，弱网环境下的状态恢复是 Ridge 差异化竞争力的关键。
3. **最后校验：真机 Remote Smoke**
   - 判定：这是物理验收手段，应在 T1 恢复、R1 指标初步建立后执行，作为迭代闭环标志。

### 二、Iteration 3 候选任务排序

#### 1. [P0] 恢复确定性检查门禁与工程护栏（T1）

- 用户价值：消除盲目开发风险，确保提交不引入破坏 Remote 稳定性的类型错误。
- 最小代码范围：重构 `packages/remote` 与 `ridge-core` 的 `tsconfig` 实现增量检查；修复 `svelte-check` 性能瓶颈或改为按需检查；修复 Vite `chunkSizeWarningLimit` 位置错误。
- 自动验收：`pnpm check --filter ./packages/*` 在 120 秒内 exit 0。
- 停机条件：若优化后仍超过 180 秒，停止全量扫描，改为仅在 CI 运行针对变更文件的隔离检查。
- 不做：不升级核心依赖；不重构业务逻辑；不处理 Rust `rustfmt` 历史问题。

#### 2. [P1] 弱网与长时稳定性证据化（R1）

- 用户价值：确保用户在电梯、换网或长时间切后台后，Remote 恢复正确状态，不串 Pane、不丢历史。
- 最小代码范围：集成 Network Link Conditioner 或 CI 协议干扰脚本；在 `RemoteLink` 增加连接健康度遥测（RTT、丢包率、重连次数）。
- 自动验收：5% 随机丢包下 DataChannel 历史同步成功率 100%；重连后终端 Grid 校验码一致。
- 人工确认：真机切 Wi-Fi/4G 后 3 秒内恢复当前 Pane 实时输出。
- 停机条件：若恢复时间稳定超过 5 秒，则停止新功能，转入传输层协议重构。
- 不做：不重写 WebRTC 数据平面；不做大规模传输算法优化。

#### 3. [P1] Remote Agent 控制台 MVP（P1）

- 用户价值：把 Remote 从远程终端提升为控制平面，快速识别谁在工作、谁卡住。
- 最小代码范围：在 `packages/remote` 引入只读 Roster；点击成员触发 `switch_pane`；展示最近熔断/异常摘要。
- 自动验收：Vitest 验证点击成员触发带 `paneId` 的调用。
- 人工确认：移动 Safari 点击 Worker A，主视图切换到对应 Pane。
- 停机条件：若移动渲染内存超过现有硬上限，则取消实时头像/动画。
- 不做：不支持任务编辑、复杂拓扑图、MCP 动态端点发现。

### 三、规划决策建议

- iteration 3 主目标定义为“从功能实现转向证据验证”。
- 真机 Smoke 自动化部分包括 capability 合同、静态资源路径；人工部分包括触屏手势冲突与真实换网体验。
- 若 T1 无法在前 3 天闭环，推迟 Agent UI，避免在不可靠检查器下增加 UI 状态流。

NotebookLM 使用来源：iteration 2 报告 `1ee860a4-81b4-45d6-9ed7-65de0301d618`、项目状态/差距/愿景来源、iteration 1 报告。

## 对抗评审

独立 checker：`iteration_3_adversarial_review`；只读审核，未修改仓库。

| NotebookLM 候选 | 裁决 | 关键理由 |
| --- | --- | --- |
| `pnpm check` 工程门禁 P0 | 修改采纳，降为短前置 | 一次 300 秒超时不足以升 P0；`--incremental` 已在 24.6 秒完成 70 files、0 errors、exit 0。NotebookLM 的 `pnpm check --filter ./packages/*` 实测无效，selected packages 也没有 `check` script。Vite warning 与 checker 无因果关系，不夹带。 |
| 弱网/长时 fault-injection | 修改采纳，作为 iteration 3 唯一主目标 | 风险与现有重连/背压实现均有代码依据，真正缺口是跨层、可重复证据。随机 5% 丢包、NLC、三秒真机恢复都不是跨平台确定性 CI 信号；应改为显式故障时序、fake timers、固定 pane/字节 fixture。 |
| Remote Agent 控制台 | 本轮驳回 | `get_teammate_topology` 仍是本机 Tauri IPC，不在 Remote allowlist/core/rdg；`switch_pane` RPC 不存在；roster 隐私字段、授权、断线回放均无合同。先另起只读 topology 安全设计，不能直接做 UI。 |

### 经检验后采纳的 iteration 3 方向

唯一主目标：建立 Cloud Remote 重连、背压与状态恢复的确定性跨层 fault-injection 门禁。

最小覆盖：

1. RTC failure 立即以 `RpcReconnectError` settle 在途 RPC。
2. 信令存活时只走 ICE restart；恢复后 `$/hello` 与 pane 订阅各一次。
3. 信令与 RTC 同时失败时重建 PC/DC、重跑 E2EE/授权并恢复订阅。
4. 高水位丢 pane 增量，drain 后每个受影响 pane 恰好一次 resync，不串 pane。
5. fake timers 下固定执行 100 次失败/恢复周期：无 pending request、重复订阅、残留 timer；发送次数线性且精确。

边界：优先只改测试；若必须增加 seam，只准最小可注入 seam。不得新增 RPC/协议/产品遥测/随机网络依赖，不做传输层重构。真机换网仅作非阻断补充证据。
