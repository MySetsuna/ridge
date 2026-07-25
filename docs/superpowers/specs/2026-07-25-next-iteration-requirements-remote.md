# 下个迭代需求（待实现）—— Remote 收敛收尾 + P4 真机验证

- **日期**: 2026-07-25
- **状态**: 需求稿（待实现；本轮 v0.1.1 已发布后据「已做 vs 待做」差集提炼）
- **来源**:
  - 本轮 v0.1.1 交付（后端三腿收敛 + P4 手机保活 + resume/增量基础），见
    `2026-07-25-remote-ws-pane-convergence-design.md`、`2026-07-25-P4-mobile-keepalive-change-log.md`、
    `2026-07-16-remote-frontend-unify-and-mobile-keepalive-design.md`。
  - NLM 最近两次对话记录（`docs/iterations/2026-07-24-notebooklm-guidance-31.md`/`-30.md` +
    `2026-07-24-open-planning-note-from-nlm-conversations.md`）：该弧 **open=0 全 implemented**
    （多 host / 出站 PTY / 终端链接 / git 护栏），本轮需求**不重复**这些已闭合项。
- **NLM live 同步状态**: ✅ **已完成**（2026-07-25，用户 `nlm login` 后经外部 CDP 抽 cookie 认证）。
  本文已作为「项目现状」source 上传笔记本 `66919cb9-…`（source `59c7525b`）;并经 `notebook_query`
  对抗评审（conversation `a47d3199`）—— 评审印证清单，两处修正已并入下方（R-TESTGATE 升 P1、
  R-VERIFY 复用既有 evidence 脚本、补减法机会 §1b、R-P4-LRU 验收具体化）。

## 0. 已做（筛除项——下轮**不再做**）

- 后端 pane 服务三腿（桌面 LAN / 桌面 cloud / rdg）帧+resync 收敛到 `ridge_remote::pane` 一份 SSOT。
- `ridge_term::ModeTracker` 一份 modes 追踪；rdg 补 live modes → 修 rdg TUI 鼠标。
- `get_pane_resync_preamble` 补 cloud 首订阅模式前导 → 修公网手机 TUI 鼠标。
- P4 手机端弃单例 kernel + 旁路缓存,接共享 `manager` 多 kernel park 保活(svelte-check 0-error)。
- `resume`(无RIS 续订)全链路激活 + `PaneScrollback::since`/`sinceSeq` **后端**增量 replay 基础。
- v0.1.1 发布(11 资产齐全)。
- **R-CLOUD-CONVERGE 已闭（v0.1.2，2026-07-25）**: host 新增 `get_pane_resync_frame` 出一份完整 resync 帧
  （`build_resync_frame` SSOT），cloud 控制端原样喂；删前端自拼帧 + 空转的 `get_pane_resync_preamble`。
  **顺带修真 bug**：旧前导命令只加进 `CORE_MIGRATED_METHODS`（路由表）而非真能力门
  `capability.rs::REMOTE_ALLOWLIST`/`remoteAllowlist.ts` → cloud invoke 被拒 → v0.1.1 云路径鼠标修复实为空转；
  今入真门修复。详见 `2026-07-25-R-CLOUD-CONVERGE-change-log.md`。

## 1. 待做 / 待修（优先级 + 确定性验收）

### P0 — R-VERIFY: P4 保活真机验证（最高优先,可能回归)
本轮 P4 + resume **仅代码/类型级信心,运行时无 headless 测**。须在能跑 app 环境（`tauri:dev:cdp`,
非提权计划任务法见前端设计 R5.3）逐项验 `2026-07-25-P4-mobile-keepalive-change-log.md` R1–R8:
切 pane 白屏 / scrollback 保真 / 软键盘 offset / IME / 选择即鼠标 / 内存 / 弱网重连 / copy pill。
- **验收**: R1–R8 各有 CDP 截图/断言通过;**复用 iteration-4 既有证据基建**——把 R1–R8 真机 CDP 结果落
  evidence JSON，`node scripts/validate-remote-smoke-evidence.mjs` 对其 exit 0(勿另造校验)。发现回归即按台账回退基线 `b9031a0` 或定点修。

### P0 — R-INCR: 增量 replay 前端激活（消 resume-live-only 的 gap 丢失）
现 `resume` 为 live-only:切回不清 kernel,但**离开期间该 pane 的输出不回放**(gap)。后端
`sinceSeq`/`since()` 已备但前端未接。需:live 二进制帧携 seq(或控制端按帧长推进游标)、控制端
按 pane 记游标、切回发 `sinceSeq` → host 增量补 gap(无RIS)。**难点**:live 帧与 resync 帧对
控制端不可辨(都 `pane_frame`),须加 seq 标注或 meta 区分,处理 desync-resync 后游标重置。
- **验收**: 切走→pane 产出→切回,历史**连续无 gap 无重复**(单测 host `since()` 边界 + 前端游标推进纯逻辑测);
  meta `incremental`/`headSeq` 被消费。**属改线协议,须真机验**——设计 §8.4,rdg `Ring::since` 为样板。

### P1 — R-WSLEG: WS-leg 完整 trait 收口（本轮只收了帧/resync 子集）
桌面 `remote_host_impl.rs::handle_ws`(~1300 行)仍非共享实现;`host.rs` 注释所指
`PaneProvider`/`InvokeDispatcher`/`EventBus` trait 化仍未做。把每连接 WS 会话收口进 `ridge-remote`,
桌面/rdg 各出 adapter,消除两套 select 循环。
- **验收**: 共享 `serve_pane_session` 泛型驱动桌面+rdg;`cargo check -p ridge -p ridge-cli` 绿;桌面 LAN/cloud 回归不破。
- **风险/边界**: 深耦 AppState(client registry/事件广播/global-ws/限流/invoke),**分步**、每步门禁绿;勿一次性大爆炸。

### P1 — R-RDG-INCR: rdg 增量 replay 接线
rdg `ScrollbackRing::since(cursor)` 现成但只接了 `resume` live-only。随 R-INCR 一并接 `sinceSeq` 增量。
- **验收**: rdg host `since` 路径单测 + 与桌面同游标语义。

### ~~P2 — R-CLOUD-CONVERGE~~ ✅ 已闭（v0.1.2，见 §0）

### P2 — R-DESKTOP-RESYNC: 桌面 RidgePane 首屏也收敛到完整帧（R-CLOUD-CONVERGE 派生）
桌面 `RidgePane.svelte` 首屏 mount 仍经 `get_pane_scrollback_tail` 无前导 seed 内核；重挂时若 TUI 一次性
开启（`?1002h`/`?1049h`…）已滑出 tail → **桌面本地 pane 亦可能鼠标失灵**（与 cloud 同源病，本轮只修了云腿）。
收敛为 mount 也调 `get_pane_resync_frame`（复用本轮 host 命令），前端只喂;随后评估删
`get_pane_scrollback_tail`(若无其他调用方)。
- **验收**: RidgePane 首屏经 `get_pane_resync_frame`;桌面 TUI 重挂鼠标存活(真机验，属桌面主路径高影响，独立提交);相关 vitest/svelte-check 绿。
- **风险**: 桌面终端是最高频路径，回归影响大 → 分步、门禁绿、勿与其他改动混提。

### P2 — R-P4-LRU: 手机保活内存 LRU 兜底（设计 §8.2 未落地）
现所有看过的 pane kernel 常驻(仅 host 关 pane 才 detach),低端机开大量 pane 有 OOM 风险。补 LRU 上限
(默认 N=8)+ 逐出者轻量冻结(≤64KB 尾)→ 切回 rehydrate。
- **验收**: vitest 断言开第 9 个 pane 后自动逐出、`manager` 存活 kernel 数 ≤ 8(纯逻辑 LRU 测);真机 heap ≤ 预算(app 验)。

### P2 — R-P5P6: 手机壳/面板迁包 + 清理（前端统一收尾）
`src/remote/*` → `packages/remote/src/mobile/`;`src/lib/remote/RemotePanel` → `panel/`;删死路径。
承接 2026-07-16 设计 P5/P6。
- **验收**: `build:remote` + `build:desktop-web` 产物一致;`svelte-check` 绿;grep 无残留旧路径。

### P1 — R-TESTGATE: 测试门禁补强（NLM 评审升 P3→P1）
> **NLM 裁决(采纳)**: 低频维护态下「保全链绿」的自动化比部分 P2 优化更高杠杆;本轮 `worker.format`
> 构建挂(svelte-check 过、构建期才炸、耗一轮 CI)正证明现流水线**无法把「代码级信心」转成「发布级信心」**。故升 P1。

本地 node_modules/pnpm-store 不全致 vitest/vite 无法本地跑(仅 svelte-check + cargo 可用);release.yml 只
跑 build 不跑 test。补:CI 加 test job(`vitest run` + `svelte-check` + 关键 `vite build` 冒烟),红则阻发布。
- **验收**: `release.yml`(或前置 CI)含 `vitest run` + `svelte-check` step,失败阻断发布。

## 1b. 减法机会（NLM 评审补充,与加法同权）
- **删 cloud 前端自建帧**: R-CLOUD-CONVERGE 落地后,立即删 `cloudRemote._subscribe` 手拼 `RIS+前导+tail`
  的旧逻辑(host 出一份 resync 帧,前端只喂)。验收 = grep 无该拼接 + cloudRemote.test.ts 绿。
- **T3 脚本合并**: `scripts/check-prod-status.mjs` 若长期桩验、无真机实跑,并入 CI 发布脚本,减独立维护。
- **E1 遥测清理**: WebGPU 收益测量(E1)若连续 ~10 轮无真机数据录入,删其性能追踪冗余遥测,仅留运行时探测。

## 2. 边界（不做）
- 不做云 WebRTC 出站二期以外的新出站形态(NLM 弧已定 LAN WS 首切片,云出站二期)。
- 不为对标/炫技加功能;不引入 CRDT/pairing/daemon(NLM 已驳回先例)。
- 不改已闭合的 multi-host/git-guard/终端链接 SSOT(open-planning-note open=0)。

## 3. 建议轮次编排
0. **R-TESTGATE 先立**(CI test/build 冒烟门禁——便宜、高杠杆,先护住后续每轮「全链绿」,免再出本轮 CI 挂)。
1. **R-VERIFY**(跑 app 验 P4,本轮盲改的账,最高回归风险,先兜住)。
2. R-INCR + R-RDG-INCR(增量 replay,真机验)。
3. R-WSLEG(分步 trait 收口)。
4. R-CLOUD-CONVERGE(+随后删 cloud 自建帧)+ R-P4-LRU + R-P5P6(收尾)。

## 4. NLM 闭环状态
- ✅ 认证(2026-07-25 外部 CDP)、现状 source 上传(`59c7525b`)、对抗评审(conv `a47d3199`)、据裁决修订(本文)均已完成。
- **下轮起点**: 建 `docs/ARCHITECTURE.md`(本仓无,据 codegraph 生成 Remote 现状)作规范「现状」来源;之后每轮按
  skill ⑦ `source_delete` 旧现状 + 传新,替换 `59c7525b`(本需求稿属一次性现状快照,非常设 ARCHITECTURE)。
