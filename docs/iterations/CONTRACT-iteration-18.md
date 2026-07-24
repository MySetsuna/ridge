# CONTRACT — Iteration 18（双报告愿景闭合）

## 目标

笔记本两份深研源（Actionable Brief + Architectural Blueprint）精华落地：WI 全表 **implemented | rejected**，**open=0**；研究源删除；来源恒 PROJECT-STATE。

## 边界

- 不重写 15–17 已闭合切片；不把已驳回项重开为 open。
- 真机/生产云/merge 非本合同。
- 完整 WS 出站 PTY 保持下一里程。

## 验收

| # | 信号 |
| --- | --- |
| 1 | `docs/iterations/2026-07-24-open-vision-checklist-dual-report.md` open=0 |
| 2 | `cargo test -p ridge --lib` 子集：hosts / teammate / reconnect_policy 全绿 |
| 3 | F2：`suspend_with_os` 经 `job_object::try_freeze_primary`；测覆盖 freeze 入口 |
| 4 | `nlm source list` 仅 PROJECT-STATE；note `[已实现]` |

## 停机

确定性闸红；或研究源删前 open>0。
