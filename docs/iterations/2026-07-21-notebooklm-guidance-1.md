# NotebookLM guidance — iteration 1

日期：2026-07-21  
Notebook：`Ridge 项目现状、愿景与规划基线（2026-07-21）`  
会话：`a47d3199-c1f9-47f1-927c-ff2c4875b77d`

## NotebookLM 原始建议摘要

NotebookLM 基于 `PRODUCT_VISION.md`、`GAP_PORTFOLIO.md`、`PROJECT_STATUS.md` 与 iteration 1 报告给出：

1. P0：恢复可信开发基线与协议归一。恢复完整质量门禁，并消除 wind/ridge-cloud 的协议双 SSOT。
2. P1：跨入口能力矩阵自动同步与校验。以 Rust 为 SSOT，显式验证 desktop、LAN、cloud、rdg 的支持/拒绝/降级。
3. P1：Remote Agent 控制台 MVP。移动端显示 roster、状态、异常，并可切到成员 Pane。

它将 `get_workspace_snapshot` 漂移判为系统问题；认为弱网/长时风险高于真机 smoke；将 Vite/PWA 构建警告判为 P2。它建议 iteration 2 以“可验证的跨入口一致控制平面基线”为目标，但合同草案同时混入门禁、协议、能力矩阵和 Agent UI 三个里程碑。

## 来源核对

- `GAP_PORTFOLIO.md` 与 `PROJECT_STATUS.md` 确实支持跨入口能力矩阵、协议单一权威和 Remote Agent 控制台方向。
- `PROJECT_STATUS.md` 的“pnpm/Rust toolchain 不可见”已被本轮现实推翻：pnpm、Cargo、wasm-pack 均可启动，定向测试、WASM 构建与 Remote bundle 均 exit 0。`pnpm check` 额外运行 180 秒无诊断后超时，故完整门禁仍未证明全绿，但问题不再是工具不可见。
- 两份协议文件仍真实存在且内容不同：wind 423 行，ridge-cloud 521 行，SHA-256 不同；这是有效债务，但跨仓删除/改权威入口超出本轮 Remote 终端修复范围。
- 现有 `packages/remote/src/shared/transport/conformance.test.ts` 已覆盖 LAN-WS 与 cloud-WebRTC 传输合同，本轮复跑 32/32 通过；缺口不是“完全没有 conformance”，而是能力宣告、方法路由与 UI 消费未形成同一合同。

## 对抗评审

| 建议 | 裁决 | 理由 |
| --- | --- | --- |
| 恢复工具链作为 iteration 2 主目标 | 驳回 | 工具链已可见且本轮核心闸绿；完整 `pnpm check` 的有界性另记风险，不能继续引用旧来源声称工具缺失。 |
| 消除 Cloud 协议双 SSOT | 部分采纳、延期 | 债务真实，但属跨仓协议治理；需单独授权与合同，不与 Remote 能力门禁混做。 |
| 跨入口能力矩阵自动化 | 采纳并升为 iteration 2 唯一主目标 | 本轮已出现 `get_workspace_snapshot` 活跃漂移；现有测试只钉 Rust↔TS allowlist 与两条传输，未证明“宣告→路由→UI”闭合。 |
| Remote Agent 控制台 MVP | 战略采纳、本轮驳回 | 产品价值成立；但 teammate topology 仍是桌面能力，直接做 UI 会扩大 Remote capability 漂移。 |
| 弱网/长时高于真机 smoke | 保留风险、驳回确定排序 | 两者都缺当前测量；弱网实现已有大量单测，尚无数据证明其必然高于真机兼容风险。 |
| 构建警告 P2 | 采纳 | `chunkSizeWarningLimit` 确实误放在 Rollup output options，PWA glob 也有空匹配；构建仍成功，不抢占主目标。 |

## 最终采纳

Iteration 2 只建立“跨入口 Remote 能力合同门禁”：同一份可执行合同覆盖能力宣告、安全放行、实际 handler 与 Controller UI 降级。其余建议进入后续候选，不并入本轮。

## 后续授权与处置

用户随后明确授权跨仓修改。`C:\code\ridge-cloud` 的干净 `develop` 执行 `git pull --ff-only` 后确认已是最新；权威协议无需改内容。wind 的陈旧协议全文已替换为指向 ridge-cloud canonical 的短入口，并加入 Vitest 守卫，防止本仓再次维护第二份协议正文。该独立治理项已完成，不并入 iteration 2 的能力合同范围。
