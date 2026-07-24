# CONTRACT — Iteration 21（维护态 / 可选）

## 目标（可选，无强制大迭代）

1. 静态确认 git 旁路：`packages/ridge-core` 内 `Command::new("git")` / 裸 `.output()` 不绕过 `git_output`（允许测试与 taskkill）。
2. 用户轨：T3 生产 status 实跑、真机 smoke JSON。

## 验收

| # | 信号 |
| --- | --- |
| 1 | 可选：`rg`/`cargo test` 旁路门禁绿 |
| 2 | 用户轨证据进 scratch/artifacts，非自动宣称 |

## 停机

引入新状态源或空 Release。
