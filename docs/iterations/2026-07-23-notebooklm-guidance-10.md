# NotebookLM 指导 — iteration 10 之后（规划 iteration 11）

日期：2026-07-23 · 来源：单一来源 `PROJECT-STATE`（source `7169f5c8`，iteration 10 后版本）

## NotebookLM 原文要点

- Q1：主线 = **M1 切片一**（suspended panes 持久化 + 启动恢复；补 G1「重启即失」，零协议面）。
- Q2：自动轨收敛后转**低频维护态**；红线 = 无用户轨证据不启动扩协议面/新状态源/改 E2EE 的功能开发，违者即「制造工作」驳回。
- Q3：M2 依赖已满足（agent_id 自 iteration 6 稳定定档）；M2 可与 M1 切片二合并为「上下文持久化增强」轮。
- Q4：G1 阶段二 Windows 竞态缺口**接受为文档声明的已知边界**（Ridge 是控制平面非加固沙箱）。
- 三目标：M1 切片一（验收含「ridge-core 自动加载 sidecar」「关区写入延迟 <200ms 停机线」）；P2 阶段 2 设计文档（`docs/designs/hitl-resolution-v2.md`）；导读刷新维护。

## 对抗评审（代码事实 checker）

### 采纳

- **Q1 M1 切片一主线**：✔ 读写方已在（suspend/resume 即写方、启动恢复即读方），设计已给验收路径。
- **Q2 低频维护态 + 制造工作红线**：✔ 原文表述准确，录入循环纪律。
- **Q4 Windows 竞态缺口文档声明**：✔（G1 阶段二本轮仍不排期，裁决先立此存照）。
- **目标 2 P2 阶段 2 设计文档**：✔ 设计不扩协议面，积压期可做；范围含 nonce 防重放/单次消费/过期/多 controller 冲突/审计规范/传输面选型。
- **目标 3 维护节律**：✔ 已固化为 WORKFLOW 常规。
- **Q3 M2 合并建议**：方向合理，**留待 M1 切片一落地后**的下轮合同裁决（本轮不并——一轮一主线）。

### 驳回/修正（附代码事实）

1. **「ridge-core 自动加载 sidecar」——层错**：suspend 注册表在 `src-tauri/src/teammate/suspend.rs`（进程级，iteration 9），sidecar IO 与启动恢复同属 src-tauri（app 数据目录经 `state.app_handle`）；ridge-core 零涉。验收改 `cargo test -p ridge --lib`。
2. **「关区写入延迟 >200ms 停机」——不可判定**：本仓无性能门禁，时延断言在单测环境非确定。改确定性停机/韧性条款：**sidecar IO 失败 fail-open**（暂停语义继续、log warn、不阻断关闭/启动）；损坏 json 启动不 panic（设计 §3 已定）。
3. **`docs/designs/` 路径**：仓惯例为 `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md`。

### 结论

iteration 11 = M1 切片一实现（主线）+ P2 阶段 2 设计文档 + 维护节律，三目标（存量收敛，目标数缩减合规）；见 `CONTRACT-iteration-11.md`。
