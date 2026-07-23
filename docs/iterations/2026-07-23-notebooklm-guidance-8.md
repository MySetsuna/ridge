# NotebookLM 指导 — iteration 8 之后（规划 iteration 9）

日期：2026-07-23 · 来源：单一来源 `PROJECT-STATE`（source `18daec4b`，iteration 8 后版本）

## NotebookLM 原文要点

- Q1：主线 = **G1 阶段一实现**（不扩协议面正确；本轮定义为「内部功能对齐与债务清偿轮」）。
- Q2：**E1 执行代码清理（删 WebGPU 实验代码）**；E2 关闭待证据。
- Q3：审查辅助包**是**自动轨交付物（降低 35+ 提交合并认知负荷属工程可控性）。
- Q4：C1 半自动收口高度可行（用 A2 矩阵列差值，不强行对齐代码）。
- 六目标草案：G1 软暂停（`AgentStatus` 状态机 + `soft_pause`，停机条件 Windows Job Object 死锁）；E1 清理（grep 无激活路径 + `pnpm build` exit 0）；C1 清单化（矩阵补齐 rdgHost 列 + `pnpm test:conformance` + GAP-REPORT）；审查包脚本（bash + 正则分类）；A1 workspace 写路径审计文档；M1 最小结构体定义（6 字段 serde，无持久化）。

## 对抗评审（codegraph/代码事实 checker）

### 采纳

- **Q1 G1 阶段一主线**：✔ 与已定稿设计文档一致；不扩 Remote 协议面契合用户轨积压现实。
- **Q3 审查辅助包**：✔ 采纳为自动轨交付物；实现改 node .mjs（Windows 环境，仓内脚本惯例），分类依据 conventional commit type/scope + 协议面路径探测，非脆弱正则堆。
- **Q4 C1 半自动**：✔ 方向采纳。修正其事实错误：`capability-matrix.json` **已有完整 rdgHost 列**（iteration 5 起 6 入口全列）；`pnpm test:conformance` 脚本不存在（一致性测试本就在 vitest 全伞）。收口物 = 从矩阵派生的 rdg 语义缺口报告。
- **E2 关闭待证据**：✔ 纯簿记。
- **A1 workspace 写路径审计（仅文档）**：✔ 低成本预研，明确零代码变更。

### 驳回（附代码事实）

1. **E1「删除 WebGPU 实验代码」——重大驳回**。代码事实：`packages/ridge-term/Cargo.toml` `default = ["webgpu"]`——WebGPU 是**生产默认特性**，`RenderHandle.newWithWebgpuFirst` 运行时探测 GPU、不可用自动回退 Canvas2D；Cargo.toml 注释明载 **2026-05-05 用户反馈**：「不要 build flag 或 localStorage opt-in，运行时探测驱动」。渲染协议（`RendererBackend = 'webgpu' | 'canvas2d'`）、worker 测试、RidgePane/paneTree 均含其激活面。删除 = 拆用户钦定的默认生产路径，且与其所引 PROJECT-STATE「Canvas2D 是生产主路径、WebGPU 属实验」的**陈旧描述**互为因果——错在状态文档措辞，不在代码。**正解：E1 簿记校正**（重定义为「真机 GPU 收益测量」，属用户轨；状态文档措辞更新），零代码删除。
2. **M1 最小结构体定义**：无消费者、无持久化的 struct = 死脚手架，与上轮驳回「Suspended/Resuming 死枚举」同理（YAGNI）；其自己的减法方案（不持久化不同步）恰证明该结构本轮无用。M1 维持未做，待真实消费者出现。
3. **G1 停机条件「Windows Job Object 死锁」**：混淆阶段——Job Object 属设计的阶段三；阶段一（输入门控）纯 Ridge 内零 OS 冻结。真实风险是「agent 写路径入口不唯一（绕过门控）」，合同已改。
4. **`pnpm build` exit 0 / `pnpm test:conformance` 作验收**：前者过重（全量构建非门禁），后者不存在；一律用既有门禁（cargo/vitest/svelte-check）。
5. **审查包 bash + 正则 + 停机条件「提交记录过于混乱」**：本分支提交全为 conventional 格式（type(scope)），无混乱可言；bash 在 Windows 环境非首选，改 node .mjs。

### 结论

iteration 9 = G1 阶段一实现（主线）+ C1 缺口报告 + 审查辅助包 + E1/E2 簿记校正 + A1 写路径审计文档，五目标；见 `CONTRACT-iteration-9.md`。
