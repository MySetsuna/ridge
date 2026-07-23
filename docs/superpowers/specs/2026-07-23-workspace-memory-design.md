# Workspace Memory 最小设计（M1，iteration 10 G2——仅设计，零代码）

日期：2026-07-23。动机（iteration 9 后成立的门槛论据）：G1 暂停态与 P2 审批数据当前皆进程态，重启即失，与「可续」愿景冲突。本文定 6 字段语义、持久化落点与首批真实读写方；**实现随首个读写方同轮出现**（不预写 struct——两度驳回的死脚手架教训）。

## 1. 六字段（最小集，宁缺毋滥）

| 字段 | 语义 | 首个写方 | 首个读方 |
| --- | --- | --- | --- |
| `goal` | 工作区当前目标（人写一句话） | 桌面 UI（后续轮） | Agent Center 展示 / 未来注入 agent 上下文 |
| `constraints` | 锁定约束列表（如「不动 X 目录」） | 同上 | 同上 |
| `decisions` | 追加式决策记录（时间戳 + 一句话） | HITL 裁决路径（approve/reject/modify 摘要） | 审批历史展示 |
| `tasks` | 任务清单（id + 状态） | delegate 路径（派活即记） | roster/汇报 |
| `runtime` | 运行态恢复数据：**suspended panes 集合**、HITL 网关开关 | suspend/resume、set_hitl_enabled | 启动恢复（重启后重挂暂停态） |
| `updatedAt` | 最后写入时刻（ms） | 每次写 | 陈旧度判断 |

隐私边界（硬约束，沿 HITL 投影纪律）：`decisions` 只存**风险分类 + reason 摘要 + 裁决**，绝不存命令全文；全字段不落 token/密钥/env。

## 2. 持久化落点选型

| 候选 | 评估 |
| --- | --- |
| A. 扩 `.ridge` 文件加 `memory` 段 | 与既有 save/restore/auto-save 同管道；但 `.ridge` 是**用户显式保存**的布局文件，未保存工作区无文件（G1 暂停态恰恰在未保存区也需恢复）；且 memory 高频写（每次裁决/暂停）会放大 auto-save IO 与冲突面 |
| B. 独立 sidecar：`{app_data}/workspace-memory/{workspace_id}.json` | 写路径独立、未保存工作区可用、损坏只失忆不坏布局；需自管清理（工作区关闭即删——恰可挂在 iteration 10 已同源化的 `close_workspace_core` 单点） |
| C. SQLx/数据库 | 本仓桌面端无 DB 依赖，引入即新状态源，违决策过滤器 |

**选 B**。理由：与布局文件解耦（写频/生命周期/损坏域皆不同）；`close_workspace_core` 已单源，删除钩子一处即全径覆盖；JSON 原子写（temp+rename）即可，无迁移负担。

## 3. 读写协议

- 写：进程内聚合 + debounce 落盘（沿 `schedule_auto_save` 模式）；原子写 temp+rename。
- 读：工作区打开/应用启动时一次载入；损坏文件按「失忆非致命」处理（log warn + 重建空 memory，绝不阻断启动）。
- 并发：单进程单写者（AppState 持有），无跨进程锁需求。

## 4. 实施切片（后续合同素材，每片自带读写方）

1. 切片一：`runtime.suspendedPanes` 持久化 + 启动恢复（写方 = suspend/resume；读方 = 启动重挂）——G1 阶段 1.5，最小闭环。
2. 切片二：`decisions` 由 HITL 裁决路径追加 + Agent Center 历史展示。
3. 切片三：goal/constraints 的 UI 编辑与展示；`tasks` 接 delegate。

## 5. 验收路径（实现轮）

- 切片一：suspend → 重启进程（测试内 new AppState + 载入）→ is_suspended 仍真；损坏 json → 启动不 panic 且 memory 为空。
- 每切片 `cargo test --workspace` exit 0；无新协议面（Remote 不直读 memory 文件，仍经既有投影）。
