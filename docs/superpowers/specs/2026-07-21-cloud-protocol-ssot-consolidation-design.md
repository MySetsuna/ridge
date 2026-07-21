# Ridge Cloud 协议 SSOT 收敛设计

日期：2026-07-21

## 问题

`ridge-cloud/docs/ridge-cloud-protocol.md` 与 `wind/docs/contracts/ridge-cloud-protocol.md` 都自称“唯一权威契约”，但最新内容已明显分叉：前者 521 行，后者 423 行，SHA-256 不同。继续维护两个全文会让跨仓实现和评审引用不同事实。

`ridge-cloud` 已在干净的 `develop` 上执行 `git pull --ff-only`，结果为 `Already up to date`。因此本次以其最新协议全文为 canonical。

## 方案

1. 保留 `ridge-cloud/docs/ridge-cloud-protocol.md` 作为唯一权威全文，不复制到 wind。
2. 将 wind 的既有路径保留为短入口，包含：
   - 权威仓库与文件路径；
   - GitHub 可点击链接；
   - 本地 sibling checkout 路径；
   - “先改 canonical，再改代码/测试”的变更顺序。
3. 不批量改写历史设计、交接和审计文档。它们继续引用 wind 入口即可自动落到 canonical；历史语境不被篡改。
4. 加一个 Vitest 守卫，断言 wind 入口指向 ridge-cloud、篇幅保持为短入口、且不再自称协议正文 SSOT。

## 非目标

- 不修改任何协议字段、错误码、消息形状或运行时代码。
- 不删除 wind 的入口路径，避免历史链接失效。
- 不让测试依赖 sibling `ridge-cloud` checkout 或网络；CI 只验证入口结构。

## 验收

- wind 入口不超过 40 行，明确指向 `MySetsuna/ridge-cloud` 的 `develop/docs/ridge-cloud-protocol.md`。
- 仓库内只有 ridge-cloud canonical 全文自称“单一事实来源 / SSOT”。
- 新协议入口测试退出码为 0。
- 两个仓库工作区无意外改动；ridge-cloud 仅完成同步，不产生提交。

