# NotebookLM guidance + 对抗评审 — iteration 20b

笔记本：`66919cb9-1329-4ddf-955c-f426d15a9fe6`  
来源：sole `PROJECT-STATE`（`b55fcae3-…`）  
query 摘要：Bug4 证据是否充分；下一迭代 ≤3 目标；状态源/协议副本。

## NLM 结论（maker）

1. Bug4 护栏证据充分（guard_tests / ridge-core 328 / vitest 13 / 超时杀树 / acquire 超时）。
2. 下一迭代候选：
   - Git 旁路审计 + acquire 超时可观测计数（A1/S1）
   - T3 生产实跑（用户轨）
   - 真机 smoke 证据（用户轨）
3. 不引入新状态源/协议副本。

## 对抗评审（checker）

| 建议 | 裁决 | 理由 |
| --- | --- | --- |
| Bug4 充分 | **采纳** | 与 codegraph/源码一致：`run_command_with_timeout`、`spawn_git_blocking` acquire timeout、前端 clamp 对齐；确定性测驱动 shipped 路径 |
| Git 旁路审计 + 观测计数 | **顺延 CONTRACT-21 可选** | 价值中：防未来旁路；本轮已统一本文件出口。观测计数非用户可感知，Yagni 默认不做强制 |
| T3 / 真机 smoke | **驳回进自动轨** | PROJECT-STATE 已标用户轨；缺 token/真机不可代劳 |
| 映射 A1/S1 做 git 遥测 | **部分驳回** | A1 已关闭；强行占差距行会稀释维护态。写 CONTRACT-21 轻量「旁路 grep 门禁」即可，不升 P0 |

## 锁定

- 本轮闭环：0.0.19 Release + 文档/NLM 同步。
- CONTRACT-21：可选旁路静态门禁；主路径仍维护态/用户轨。
