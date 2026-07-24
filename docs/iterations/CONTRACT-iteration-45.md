# CONTRACT — Iteration 45 / AC4-C6（约 2 日 · Foreign history tail）

**Credit ID**: C6 · **NEW**

## 产品结果

远端会话输出保留 capped tail；attach 时 seed parser；history_pull_budget 协议提示。

## 独占主文件

- `src-tauri/src/hosts/foreign_history.rs`
- fanout 写 history；attach 时 feed seed

## 验收

`cargo test -p ridge --lib hosts::foreign_history` → gates-credit-C6.log
