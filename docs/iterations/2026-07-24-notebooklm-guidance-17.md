# NotebookLM 深研指导 + 对抗评审 — iteration 17

## 报告

- 本地：`docs/iterations/2026-07-24-deep-research-interaction-multihost.md`
- 笔记本临时源：`RESEARCH-REPORT-interaction-multihost`（实现后删除）
- 导入策略：**仅报告文件**，零网站源（`source list` 仅 PROJECT-STATE + 报告）

## 对抗评审（报告 WI 1.1–5.2 全表）

| WI | 裁决 |
| --- | --- |
| 1.1 Agents rail + effort + context | effort **驳回**；rail 可见性 **采纳**（health/badge）；context **采纳**（spawn env 注入） |
| 1.2 管道 `-r/-w` / sequence | **驳回** |
| 2.1 独立 PTY daemon | **驳回** |
| 2.2 多控仲裁 | **采纳**既有 HITL+cloud；补 reconnect 共用 |
| 3.1 Token interceptor | **采纳**既有 HITL；补 badge UI |
| 3.2 Pairing portal / 硬件 | **驳回** |
| 4.1 Tmux control-mode 全同步 | **驳回**（shim 已有） |
| 4.2 Worktree 沙箱 | **驳回** |
| 5.1 Viewport CRDT sync | CRDT **驳回**；foreign output fan-out **采纳** |
| 5.2 Watchdog | **采纳**（接 reconnect_policy 到 cloud+tmux） |

## 实现摘要

R17-CTX/HOST-*/RECONN/TEAM-HEALTH/HITL-BADGE；清单 open=0 后才删报告源。