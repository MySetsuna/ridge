# Remote 终端基线校准设计

日期：2026-07-21  
迭代：1  
NotebookLM：`Ridge 项目现状、愿景与规划基线（2026-07-21）` / note《远程终端迭代规划与质量修复指南》

## 背景

目标 note 将以下五项列为下一迭代候选：移动复制、scrollback/clear、一终端承载力、Remote Git Diff、全局滚动条与 TUI 点击。

代码审计显示 note 的关键假设已落后于当前 `main`：

- `git_diff_file` 已在 Rust canonical allowlist、Cloud TS mirror 与 core dispatch 中；
- 移动端复制 pill 已阻止 touch 冒泡，复制不再发送 `^C`；
- `ridge-term` 已实现 shell `ED 2` 清 scrollback parity，并有 Rust 回归测试；
- 移动端 Canvas 键盘偏移已改为 transform-independent，scrollback 亦有容量上限；
- 触屏已把 TUI 手势编码为 mouse press/motion/release；
- 全局滚动区已有 `overlayScroll` 主题与原生 fallback。

因此本轮先校准“规划来源时间”与“代码事实时间”，避免重复实现或破坏已有安全边界。

## 目标

1. 对 note 的五类建议建立代码证据、提交历史和确定性测试证据。
2. 将校准报告上传 NotebookLM，要求它基于已完成事实重排下一迭代。
3. 对新建议做来源核对和代码现实审查，再生成 iteration 2 合同。

## 非目标

- 不重新加入已存在的 allowlist 条目。
- 不扩大 WebGPU shared surface。
- 不在无可复现失败时改写输入、选择或 scrollback 状态机。
- 不把 NotebookLM 的推断当作验收信号。

## 验收

- `pnpm exec vitest run packages/remote/src/shared/cloud/remoteAllowlist.test.ts src/remote/lib/cloudRemote.test.ts src/lib/actions/overlayScroll.test.ts` 退出码为 0。
- `cargo test -p ridge-term clear_scrollback` 退出码为 0。
- `pnpm build:remote` 退出码为 0。
- 生成并上传 `docs/iterations/2026-07-21-iteration-1.md`。
- 归档 NotebookLM 原始建议、逐条采纳/驳回理由，并生成 `CONTRACT-iteration-2.md`。

## 停机条件

- 若任一确定性检查失败，则 iteration 1 转为修复首个失败，不继续向 NotebookLM 宣称基线已完成。
- 若 NotebookLM 仍只重复已修事项，则记录来源陈旧熔断，并以当前代码证据生成最小后续合同。

