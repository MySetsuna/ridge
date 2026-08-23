# Ridge SpecTree 迭代流

`specs/` 与 `changes/` 乃当前语义权威；`docs/REQUIREMENTS-SPEC.md` 及
`docs/iterations/` 留作历史证据，不再驱动 NotebookLM 冷循环。

## 开工

1. `git status --short`、`git diff` 查用户改动。
2. `.codegraph/` 存在则先用 CodeGraph 定位符号、调用链与影响面。
3. `pnpm spec:status`、`pnpm spec:validate` 查图与结构。
4. 新语义增量写入 `changes/CHG-*.md`，并以 `affects` 连至相应节点。

## 实施与验收

- SpecTree 负责需求层级、影响与变更边界；代码与运行测试仍为实现事实。
- 不伪造 `APPROVED`、`LOCKED` 或外部现场证据；用户未明确批准时维持 `DRAFT`。
- 先修根因与共享出口；定向测试后跑 `pnpm check` 及适用全量回归。
- Remote 公网、实体手机、IME、后台恢复、长期 heap/CPU/网络 soak 属现场轨；
  无匹配运行产物则明记未验证。
- `pnpm spec:obsidian` 仅导出只读视图；不得在 vault 内反向编辑权威内容。

## 收尾

1. `pnpm spec:validate`。
2. `pnpm spec:status` 对账 stale/impact。
3. `pnpm spec:obsidian` 更新本地 Obsidian 视图。
4. 记录测试退出码、剩余现场闸与未验证项；发布仍另循 `AGENTS.md` 硬闸。

NotebookLM source、note、认证与其嵌入工作流自 2026-08-23 退役；历史文档仅供追溯。
