# 第五迭代周期规划：Agent控制台与连接闭环

- note_id: `71bdadda-55e2-468c-9197-1e761bb6cc5f`
- notebook_id: `66919cb9-1329-4ddf-955c-f426d15a9fe6`
- archived_at: 2026-07-24
- source: NotebookLM note export (pre-delete archive)

---

根据唯一来源《PROJECT-STATE.md》及相关迭代报告，为您制定的 Iteration 5 规划如下：

### 1. §7 开放问题回答与主线决策

*   **主线决策：选择 P1（Remote Agent 控制台 MVP）**。
*   **理由排序：**
    1.  **用户价值（最高）**：用户明确提出侧栏重编排、分组、进程识别及任务状态展示等 4 项核心需求，直接对应 P1 的“10秒可见性”愿景 [1, 2]。
    2.  **解锁力（高）**：实现 Agent 状态模型 (M2) 是后续 HITL 审批 (P2) 和复杂协作分享 (E2) 的逻辑前提 [3, 4]。
    3.  **风险降低（中）**：通过建立 Roster 白名单路由，可避免 Remote 端直接暴露底层敏感的 MCP token [5]。
    4.  **成本（中）**：桌面端已有 teammate/tmux 基础 [6]，Remote 端侧重于 UI 组件化与 RPC 路由映射。
    5.  **可逆性（高）**：UI 层交互变更不改动底层传输协议，失败后可快速回退。
*   **其他问题回答：**
    *   **并行目标**：在用户执行 R1 真机测试期间，自动化侧并行 **T1（Cargo 绿灯）** 与 **A1（路径减法审计）** [7]。
    *   **T3 形态**：形态为 `scripts/check-prod-status.sh`。利用生产只读 Token 探测 `/health`、Git SHA 和激活的 artifact 版本指针 [8, 9]。
    *   **T1 补齐**：必须在本轮补齐 Rust 侧运行证据，作为 P0 级基线不可再拖延 [10]。

---

### 2. Iteration 5 目标清单

| 目标描述 | 差距 ID | 验收信号 | 明确不做范围 |
| :--- | :--- | :--- | :--- |
| **[主线] Agent 侧栏重构与 Roster 增强**：实现分组、默认组逻辑；展示标题、任务、正在进行的进程；支持点击切换工作区并高亮 Pane [需求 2, 4]。 | **P1 / M2** | `RpcClient.onAgentUpdate` 成功分发带分组信息的 payload；Vitest 验证 `switch_pane` RPC 触发。 | 不做复杂的 Agent 能力编辑界面；不增加 3D 拓扑图 [5]。 |
| **远端主机 Live PTY 连通性闭环**：彻底完成桌面端连接远端主机的“半成品”状态，支持出站 PTY 挂载 [需求 1]。 | **H1** | `hosts.ts::connectHost` 成功实例化 PTY 并通过 `TerminalManager` 渲染；`cargo test -p ridge-cli` 覆盖连接逻辑。 | 不支持非 Ridge 协议的通用 VNC 或 RDP。 |
| **Rust 仓库全量测试绿灯**：补齐 `cargo test --workspace` 与 `ridge-cloud` 本轮运行证据。 | **T1** | 干净沙箱环境下 `cargo test` 退出码 0 [11]。 | 不修复历史遗留的 `rustfmt` 格式差异 [12]。 |
| **Pane 交互反馈与 Header 重构**：支持待审批/结束任务高亮；Header 压缩并展示多端控制端用户 [需求 5]。 | **A1 / P2** | UI 测试验证 `hitl_request` 时 Pane 边缘 CSS 变量改变；Header 根据 `RoomRegistry` 展示多用户 [13]。 | 不在 Header 中添加文件管理或设置按钮。 |
| **Agent 自动发现开关与探测逻辑**：实现对常用 CLI Agent 进程的识别并映射到侧栏 [需求 3]。 | **M2** | 开启开关后，`teammate` 列表自动注入探测到的 `claude-code` 等进程 [14]。 | 不支持动态 MCP 端点热发现。 |
| **工作区分享安全架构设计（草案）**：定义分享模式下的权限矩阵与高风险操作隔离逻辑 [需求 6]。 | **E2 / S1** | 产出 `sharing-security.md` 设计文档；验证分享连接下 `rm_workspace` 等 RPC 被静默拒绝。 | **本轮不进行** 生产环境的分享代码部署。 |

---

### 3. 减法与不做方案

*   **减法 [WebGPU 实验]**：为了集中资源解决 Agent 侧栏重构，**彻底挂起** WebGPU 的真机收益验证（E1），移除相关的构建探测 [3, 10]。
*   **不做 [PWA 配置修复]**：在基线测试全绿前，忽略非阻断的 `chunkSizeWarningLimit` 等 Vite 警告，不列入本轮任务 [15]。

---

### 4. 愿景完成里程碑地图

1.  **里程碑 1：可视基线与连接闭环 (Iteration 5)**：完成 H1 (多主机连接)、P1 (Agent 侧栏基础) 与 T1 (Rust 绿灯)。
2.  **里程碑 2：交互可控与真机证据 (Iteration 6)**：完成 R1 (真机 Smoke 归档)、P2 (远端 HITL 审批) 与 A2 (矩阵文档化) [10]。
3.  **里程碑 3：协作分享与安全加固 (Iteration 7)**：完成 E2 (工作区分享落地)、G1 (Agent 暂停恢复) 与 S1 (安全退役) [3]。
4.  **里程碑 4：深度连续性愿景 (Iteration 8+)**：完成 M1 (项目记忆) 与 A1 (最终减法审计)，达成北极星目标 [1, 10]。

---

### 5. 评审合规性确认

*   所有新增目标均映射至 §6 的 **T1, H1, P1, P2, M2, A1, E2**，未引入无关功能。
*   针对“半成品”连接（需求 1），通过 **H1** 补齐 PTY 挂载逻辑；针对 Agent 协作（需求 2-5），通过 **P1/M2/P2** 的组合拳增强可见性。
*   安全不变量守卫：分享功能（需求 6）在设计阶段即引入 `capability_mask` 概念，坚持 **S1** 定义的 fail-closed 趋势 [16, 17]。
