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
- **NLM live 同步状态**: ⚠️ **受阻**——notebooklm-mcp 认证过期（`refresh_auth`=stale）。上传本轮现状为
  新来源 + 拿一版对抗评审后的 NLM 计划，须先由用户在终端 `nlm login`（交互式浏览器登录,
  agent 无法 headless 完成）。登录后可补跑：现状 source 替换 + `notebook_query` 换向评审。

## 0. 已做（筛除项——下轮**不再做**）

- 后端 pane 服务三腿（桌面 LAN / 桌面 cloud / rdg）帧+resync 收敛到 `ridge_remote::pane` 一份 SSOT。
- `ridge_term::ModeTracker` 一份 modes 追踪；rdg 补 live modes → 修 rdg TUI 鼠标。
- `get_pane_resync_preamble` 补 cloud 首订阅模式前导 → 修公网手机 TUI 鼠标。
- P4 手机端弃单例 kernel + 旁路缓存,接共享 `manager` 多 kernel park 保活(svelte-check 0-error)。
- `resume`(无RIS 续订)全链路激活 + `PaneScrollback::since`/`sinceSeq` **后端**增量 replay 基础。
- v0.1.1 发布(11 资产齐全)。

## 1. 待做 / 待修（优先级 + 确定性验收）

### P0 — R-VERIFY: P4 保活真机验证（最高优先,可能回归)
本轮 P4 + resume **仅代码/类型级信心,运行时无 headless 测**。须在能跑 app 环境（`tauri:dev:cdp`,
非提权计划任务法见前端设计 R5.3）逐项验 `2026-07-25-P4-mobile-keepalive-change-log.md` R1–R8:
切 pane 白屏 / scrollback 保真 / 软键盘 offset / IME / 选择即鼠标 / 内存 / 弱网重连 / copy pill。
- **验收**: R1–R8 各有 CDP 截图/断言通过;发现回归即按台账回退基线 `b9031a0` 或定点修。

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

### P2 — R-CLOUD-CONVERGE: cloud 首订阅 resync 收敛
cloud 初次订阅仍是 `cloudRemote._subscribe` **前端自建** `RIS+前导+tail`(经两个 invoke 拼),与 host
`build_resync_frame` 是**两套构建**。收敛为 host 出完整 resync 帧(一份),前端只喂,消分叉。
- **验收**: cloud 与 LAN resync 帧同源(host 侧一份构建);cloudRemote.test.ts 更新且绿。

### P2 — R-P4-LRU: 手机保活内存 LRU 兜底（设计 §8.2 未落地）
现所有看过的 pane kernel 常驻(仅 host 关 pane 才 detach),低端机开大量 pane 有 OOM 风险。补 LRU 上限
(默认 N=8)+ 逐出者轻量冻结(≤64KB 尾)→ 切回 rehydrate。
- **验收**: 开 N+2 pane,逐出生效(纯逻辑 LRU 测);真机 heap ≤ 预算(app 验)。

### P2 — R-P5P6: 手机壳/面板迁包 + 清理（前端统一收尾）
`src/remote/*` → `packages/remote/src/mobile/`;`src/lib/remote/RemotePanel` → `panel/`;删死路径。
承接 2026-07-16 设计 P5/P6。
- **验收**: `build:remote` + `build:desktop-web` 产物一致;`svelte-check` 绿;grep 无残留旧路径。

### P3 — R-TESTGATE: 测试门禁补强
本地 node_modules/pnpm-store 不全致 vitest/vite 无法本地跑(仅 svelte-check + cargo 可用);release.yml 只
跑 build 不跑 test。补:CI 加 vitest job(或 release 前置 test 门禁),避免「构建期才暴露」类问题(本轮 worker.format
即一例:svelte-check 过、构建挂)。
- **验收**: CI 有 `vitest run` + `svelte-check` gate;红则阻发布。

## 2. 边界（不做）
- 不做云 WebRTC 出站二期以外的新出站形态(NLM 弧已定 LAN WS 首切片,云出站二期)。
- 不为对标/炫技加功能;不引入 CRDT/pairing/daemon(NLM 已驳回先例)。
- 不改已闭合的 multi-host/git-guard/终端链接 SSOT(open-planning-note open=0)。

## 3. 建议轮次编排
1. **先 R-VERIFY**(跑 app 验 P4,是本轮盲改的账,最高回归风险,先兜住)。
2. R-INCR + R-RDG-INCR(增量 replay,真机验)。
3. R-WSLEG(分步 trait 收口)。
4. R-CLOUD-CONVERGE + R-P4-LRU + R-P5P6 + R-TESTGATE(收尾)。

## 4. NLM 闭环补跑清单（用户 `nlm login` 后）
1. `refresh_auth` 确认 token 生效。
2. 建/更 `docs/ARCHITECTURE.md`(本仓无,须先据 codegraph 生成 Remote 现状)→ `source_add(file)` 上传为现状来源。
3. `notebook_query`(笔记本 `66919cb9-1329-4ddf-955c-f426d15a9fe6`):以本文 §1 为现状差集,请 NLM 对抗评审
   优先级 + 补漏 + 减法机会(按 skill ④双门)。
4. 据评审修订本需求稿;`source_delete` 旧现状来源 + 传新(skill ⑦)。
