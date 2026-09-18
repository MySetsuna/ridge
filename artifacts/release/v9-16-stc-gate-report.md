# v9-16 SpecTree 执行轮报告（CHG-031 收口 + CHG-032 立项）

- 日期（UTC）：2026-09-18
- 起点 HEAD：`02c732af`；终点 HEAD：`d0ba0b32`（本地提交，未 push）
- 用户指令：批准新建 CHG，先按既有授权收口 CHG-031（apply/complete 若引入新产品代码或扩大 scope 则停）；
  新 CHG 仅处理 Desktop LAN 输出订阅 + browser-ui-e2e + REMOTE-RESUME，落点优先 `lanWsAdapter.ts`；
  禁止事项全清单见用户原指令；完成后走 propose→lock→build→verify→complete，本地 commit；
  最终只输出 8 段（本文件是执行过程记录，不替代那 8 段）。
- 本轮产品代码零改动；最终工作树仅剩一个 untracked 文件（上轮报告）。

---

## 1. CHG-031 收口（按既有授权执行）

| 步骤 | 结果 |
|---|---|
| `stc apply CHG-031`（preview） | PREVIEW，added/changed/written 全空 —— specs 早已包含该提案内容 |
| `stc apply CHG-031 --confirm` | APPLIED，零写入；`git diff` 为空，**无 scope 膨胀**，符合继续条件 |
| `stc complete CHG-031` | 失败：`COMPLETE_E_NOT_VERIFIED: POLICY_E_UNAUTHORIZED_DIFF` |
| `stc lock CHG-031`（重基线尝试） | 失败：同上 `POLICY_E_UNAUTHORIZED_DIFF`（11 文件） |

阻断漂移 11 文件（`stc verify` 实证，全部先于本会话存在，非本轮写入）：
`.claude/scheduled_tasks.lock`、`.gitignore`、`.stcignore`、`REMOTE-RESUME.md`、
`scripts/api-integration.mjs`、`scripts/browser-smoke-candidate.mjs`、
`scripts/browser-ui-e2e.mjs`、`scripts/served-bundles-check.mjs`、
`scripts/smoke-candidate.mjs`、`scripts/stc-walker.test.mjs`、`scripts/tls-host.mjs`
（即 v9-14/v9-15 两次 out-of-band 提交的产物）。

结论：apply 已干净收口；complete/lock 因 pre-existing 漂移被门挡住。
强行推进需回退历史或洗白漂移，均属越权，**已按停止条件停下**。
`stc recover-lock --confirm` 不适用（要求 change 已 completed 且图 hash 过期，两条都不满足）。
`stc next` 仍显示 CHG-031 proposal 待确认（no-op apply 未清提案记录）。

## 2. 门机制取证（读 stc 源码 + 实测，非推测）

- `dist/src/compiler/index.js:createBuildManifest`（`stc lock`）：① 要求
  `approvalFor(changeId, 当前图hash)`，否则 `LOCK_E_NOT_APPROVED`；
  ② 新基线与**上一把锁基线**逐文件比对，除 `specs/`、`changes/` 外，
  凡变化路径必须命中新 `allowedPaths`，否则 `POLICY_E_UNAUTHORIZED_DIFF`。
- `allowedPaths` = 影响集（change.affects 经 parent/verified_by/produces 下行 +
  depends_on/constrains/implements/consumes 上行）各单元 `codeTargets/testTargets`
  之并集；匹配支持 glob（`shared/index.js:pathMatches`），但全仓 spec 里**没有任何
  glob target**（已 grep 确认）。
- 提案 schema（`change-engine/proposal.js`）节点只允许 `{id,level,title,parent}`，
  **不能携带 targets**，且节点须在 change 影响集内（否则 `POLICY_E_OUT_OF_SCOPE_PROPOSAL`）。
  因此：无归属节点的文件（漂移点文件、`browser-ui-e2e.mjs`、`REMOTE-RESUME.md`）
  永远进不了任何 lock —— 这是工具层面的死锁，不是操作问题。
- 关键肯定项：`specs/migration/L3-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-a4b7a712`
  的 `code_targets` 已显式列出 `lanWsAdapter.ts`（第19行）与 `wsRemote.ts`（第30行），
  故修复所需的两个产品文件**有归属、可被干净授权**；冻结的 L2 契约无需触碰。

## 3. CHG-032 立项（用户已批准 scope，gate 审批未做）

- `stc change` 生成 `changes/CHG-032.md`（DRAFT），手填 `affects` + 全文 scope：
  根因 / 双文件修复设计（subscribe 改走 `conn.subscribePane` 单发代替直发、
  无双发论证、unsubscribe 新增 `unregisterPane` 公开方法 + 照常转发、
  重连幂等重建 / `disconnect` 已有清理复用 / mobile 零影响论证）/
  自动测试计划（落现存两测试文件，不新建）/ E2E 重做计划 / 文档计划 /
  Gate 死锁现状与三选项。`stc validate` → VALID，无新增告警。
- **未执行**（故意的）：`stc approve`（LLM 不审批，待用户正式批准动作）、
  `stc lock/build/verify/complete`（未获批 + 即使获批也会撞同一漂移墙；
  机制已由 CHG-031 重锁失败实证，无需再跑一次必败命令）、任何产品/测试/脚本/文档编辑、
  任何 bundle/二进制重建。
- 本地提交（仅 draft 文件）：`d0ba0b32`，`git diff 02c732af..HEAD --stat` 仅
  `changes/CHG-032.md +95`。未 push/tag/release/deploy。

## 4. 待用户定夺（CHG-032 文件 Gate 章节亦有）

- (c1) 显式批准一次性重基线（删陈旧锁指针后重 lock；漂移被洗白的后果一并接受）；
- (c2) e2e/文档部分走锁外流程并书面记录（沿 v9-14/v9-15 先例），产品修复仍走锁；
- (c3) 暂搁置。
- 另需对 CHG-032 的正式 gate 审批（`stc approve` 须记录你的批准，我不代批）。
- 备注：`stc complete` 本轮未执行（无授权改动可吸收，不擅自关闭 CHG-031）；
  真机六类 + Native Tauri 维持 NOT_RUN/独立；BETA_READY = NO。

---

## 附录 R1 · c1 受控重基线起飞前审计（2026-09-18，用户已批准 c1）

- 旧锁：`buildId=BUILD-ec74fb2f07c1`
  `manifestHash=ec74fb2f07c1122b8d23ce8771bc73c5a4ecc5134ae15ce1096f5403c1e2876a`
  `specHash=1dab3f779de8e9a2b10603f9fd51a965a5ad501692df5553499127d47bfaaa0e`，
  baseline 2181 文件，allowedPaths 207 条。旧 BUILD 文件保留在 `.spectree/build/`。
- 本轮起点图：`graphHash=6bf5fd43e5…`、changes=33（+1 为本会话 draft `changes/CHG-032.md`）。
- `stc verify` changed 清单 = 11 漂移 + `changes/CHG-032.md`（draft 自身，lock 路径天然豁免）。
  **无第 12 个文件**，停止条件未触发。
- 11 文件逐个来源（`git log -1`，全部先于本会话）：
  `.gitignore` / `scripts/api-integration.mjs` / `browser-smoke-candidate.mjs` /
  `served-bundles-check.mjs` / `smoke-candidate.mjs` / `stc-walker.test.mjs` / `tls-host.mjs`
  → `a958e42b` (v9-14)；`REMOTE-RESUME.md` → `02c732af` (v9-15)；
  `scripts/browser-ui-e2e.mjs` → `3a590e89` (v9-15)；
  `.claude/scheduled_tasks.lock` → `4eba7261`（旧功能提交，环境自改风险见下）；
  `.stcignore` → `7a974294`（旧修复提交）。
- 本会话仓库写入仅：`changes/CHG-032.md`（已提交 draft）+ 2 个 untracked 报告
  （`artifacts/` 在 `.stcignore` 内，walker 不可见，`verify` 实证未列出）。
  **漂移中无本轮新增产品运行时代码**。
- 关键排序：draft 已使图 hash 偏离旧审批（1dab3f→6bf5fd），故重基线前先把 draft
  移出树（恢复 1dab3f、旧审批重新有效），CHG-031 收口全程树内只有 11 个确认文件。

---

## 附录 R2 · 执行结果（2026-09-18，同日收口）

- CHG-031：`lock(BUILD-43f503ca2efe)→build→verify VALID(changed none)→complete`
  成功，状态 COMPLETED。`complete` 附带 L2×4 状态 APPROVED→LOCKED + depends_on
  流式改写（语义零变化）；approvals.json 的 CHG-031 记录被工具重锚到新 hash
  （原 09-15 人工批准，备查）。旧 BUILD-ec74 文件留档于 `.spectree/build/`。
- CHG-032：归属行 `L3-OBS-SCRIPTS += scripts/browser-ui-e2e.mjs`
  （1 行，L2 零触碰，无 glob）→ approve → lock（BUILD-c16dd879614e，5/5 必需文件覆盖确认）
  → 实施修复（`lanWsAdapter.ts` sendControl 截获 + `isStringRecord` 非导出守卫；
  `wsRemote.ts` 新增公开 `unregisterPane`，其余不动）→ 单测 11 新增（46/46 含回归；
  旧码对照 4 失败，wire 兼容断言在旧码仍过）→ build（BUILD-2f4db974）
  → 全量 `pnpm test` 2093 passed/17 skipped（仅 pre-existing stc-walker 收集错误；
  `pnpm check` 1err+1warn 经 stash 对照证实 pre-existing 且文件均禁动）
  → e2e 按三层 marker 门重写（`node --check` 过；helper 正则 5/5 点测）
  → desktop bundle 重建（2m48s；Rust 未动，ridge.exe 仍为 eaf231…）
  → 浏览器执行 NOT_RUN（`certutil -user -addstore` 同一步第 3 次挂起；
  HKCU 实证仅 pre-existing mitmproxy 证；候选无残留，生产 ridge 未触）
  → verify 前发现 L3/L4-TRANSPORT 声明接口缺 `export type TerminalFrameListener`
  （HEAD 即有、被 cloudRemote 引用，声明快照陈旧；2 个观察性 spec 各补 1 行，
  无行为含义）→ 重批重锁重建（BUILD-54ec376c6cba；其间 lock(6463)→build(d36c)
  基线出现 11 文件内容差，经查 d36c 与磁盘逐字节一致且随后 `verify` 报 changed
  none，以 d36c 为准，6463 已被 supersede，其成因未完全定论、不影响结论）
  → `verify VALID/trace100%/changed none` → complete
  （BUILD-8b7b793f48da）。`stc next`：无 open change。
- REMOTE-RESUME.md 本轮未改（defer，见 CHG-032 §文档；改它必红 verify，已证）。
  v9-16 记录由 CHG-032 文件 + 本目录两份报告承载。
- 提交：本地单 commit（含产品×2、单测×2、e2e、归属/接口 spec×3、changes×2、
  L2×4 状态、锁/审批态、两份报告），未 push/tag/release/deploy。
