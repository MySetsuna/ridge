# 公网 Remote 丝滑化：懒加载 scrollback + 内存缓存 + 真·多控制端

日期: 2026-07-02
范围: host `src-tauri`(命令白名单 + cloud pane sink 重构)、`packages/ridge-core`(白名单 SSOT)、
控制端 `src/remote`(移动 PWA)与 `src/lib`(桌面 in-browser 复用)、少量 ridge-cloud 说明。
关联: [[2026-06-25-mobile-remote-scrollback-pane-switch-design]](纯客户端缓存/prune/对账)、
`docs/TERMINAL_SCROLLBACK.md`(seq 游标 scrollback SSOT)。

## 目标(用户诉求)

公网 remote 三大痛点：
1. **加载太慢、有时卡死** —— 首屏一次性把全量 scrollback 拉回来、且每次切 pane 都重拉。
2. **要懒加载 scrollback** —— 首屏只画约 1.5 屏；用户真正向上滚动时才**分批**向 host 取历史，
   直到最旧；控制端**内存缓存**各终端 scrollback，切换不重拉。
3. **真·多控制端** —— 同 IP、多浏览器（多标签页）都能各自接入并存，仅以会话 id 区分。

## 现状架构（读码确认）

公网 remote = 浏览器 controller ⇄ WebRTC E2EE DataChannel ⇄ 桌面 host，relay 只做信令。

- **relay（ridge-cloud `ws/rooms.rs`+`handler.rs`）**：房间 = 1 host + N controller，按**随机 cid**
  寻址；`cli`（每标签页 sessionStorage 随机 UUID，见 `controllerInstanceId.ts`）用于「同标签页新连接
  顶替旧连接」；`max_controllers` 付费=5、体验=1。**多控制端在 relay 层已完备**（不同浏览器 = 不同
  cli → 不顶替，并存至上限）。
- **host 应用桥（`cloudHostBridge.ts`，每 cid 一实例）**：controller `subscribe-pane` → 注入的
  `paneOutputSource`（`cloudHostPaneSource.ts`）→ `invoke('subscribe_pane_raw')` 起 live 裸字节
  fan-out + `invoke('replay_pane_scrollback_raw')` **立即广播 `RIS + 256KiB` scrollback**。
- **host pane sink（`commands/cloud_pane.rs`）**：`subscribe_pane_raw` 按 **pane（非 cid）** 全局
  幂等注册**一条** sub，转发任务 `emit('pane-raw-{pane}', {b64})` 到**整个 host WebView**；
  `replay_pane_scrollback_raw`/`resync_pane_raw` 也 emit `pane-raw-{pane}`。
- **控制端**：
  - 路径 A **移动 PWA**（`src/remote`，`cloudRemote.ts` 实现 `RemoteLink`）：单一共享内核 +
    `paneScrollbackCache`（每 pane ≤256KB 裸字节）；切 pane `resetForSwitch()`→缓存预绘→重订阅→
    host replay→`reconcileReplay` 去重。
  - 路径 B **桌面 in-browser**（完整 SPA 经 `cloudControllerBoot`+tauriShim）：直接跑真 `manager.ts`/
    `RidgePane.svelte`，**已实现** seq 游标懒加载（mount `get_pane_scrollback_tail` + 滚顶
    `get_pane_scrollback_before` → `manager.prependScrollback`）。
- **已存在但未被 cloud 复用的关键原语（`state.rs` + `commands/terminal.rs`）**：
  - `PaneScrollback`：64KiB 分块、4MiB 上限、**单调 seq** 字节存储。
  - `get_pane_scrollback_tail(pane, max_bytes) → ScrollbackChunk{bytes, start_seq, at_oldest}`。
  - `get_pane_scrollback_before(pane, before_seq, max_bytes) → ScrollbackChunk`（向上翻页）。
  - wasm `TerminalKernel.prepend_scrollback` + `manager.prependScrollback`（把历史插到 scrollback 老端）。

## 根因

1. **首屏慢/卡死**：subscribe 即 `RIS + 256KiB` 一次性灌进 controller 的 wasm vte（16KiB 分片 ×N
   + 单帧解析），弱网下大突发 → 卡顿甚至卡死；**每次切 pane 都重灌**（`reconcileReplay` 只免重绘、
   不免传输，256KiB 照样过网）。
2. **无懒加载**：cloud 路径根本没用 seq 游标 API。**关键**：`get_pane_scrollback_tail` /
   `get_pane_scrollback_before` **不在 `REMOTE_ALLOWLIST`**（`capability.rs` / `remoteAllowlist.ts`）
   → controller 调用被桥拒 → 路径 B 的 mount 尾replay + 滚顶翻页在 cloud 下**静默失败**（命中
   RidgePane 的「旧 host 不支持 → 当作没有更多历史」catch），退回 host 的 256KiB dump。
3. **多控制端「似乎不行」**：relay 层其实支持，但 **host pane sink 按 pane 全局广播** 造成：
   - 第 2 个 controller 订阅同一 pane → 其 `replay_pane_scrollback_raw` 的 `RIS+256KiB` **广播**给
     所有 controller → **把第 1 个 controller 的屏幕冲掉重绘**（互相打架，像「只支持一个」）。
   - 第 1 个 controller 关闭 → `unsubscribe_pane_raw(pane)` 拆掉**唯一** sub → 仍在看的第 2 个
     controller **live 断流**。
   - `resync_pane_raw`（背压自愈）的 RIS 同样广播 → 跨控制端互扰。
   - （体验用户 `max_controllers=1` 是计费策略，非本设计范围，仅备注。）

## 方案（更优解：让 cloud 复用既有 seq 游标 scrollback）

**核心决策**：把「历史」与「实时」彻底分离——
- **历史**（首屏 + 向上翻页 + 切回补历史）：一律走**控制端主动拉**的 seq 游标命令
  （`get_pane_scrollback_tail`/`before`），在**各自内核本地渲染**。
- **实时**：`subscribe_pane_raw` 只 fan-out **新到字节**（广播，所有观看者都要 live）。

好处：直接命中三诉求，且**天然满足多控制端隔离**（host 不再推 RIS，各 controller 只 RIS 自己的内核）；
最大化复用桌面已验证原语（DRY）；去掉 cloud_pane.rs 的 replay/desync/resync 一大坨广播机制。

### 首屏 seam（历史 vs 实时无缝拼接）

`subscribe_pane_raw` 注册 sub 时**同刻**返回边界 `head_seq`（store 的下一个写入 seq）。因为 sub 在注册
**之后**才收到字节，故：
- 实时 = seq ≥ `head_seq`（fan-out 只推注册后新字节）；
- 历史 = `get_pane_scrollback_before(pane, head_seq, budget)` = seq < `head_seq`。

两段以 `head_seq` 为界、**无重叠无空洞**：先把历史喂进 grid（空闲 pane 也能画出「最后一屏」），实时字节
自然接在其后。注册与读 `head_seq` 在 PTY 读循环的**块边界**天然对齐（fan-out 与 append 同在读循环、块原子），
故边界精确到块；极端 µs 竞态最多 1 块（≤数 KB）重复，自愈，且远优于今天「每次切 256KiB 卡死」。

> 备注：`subscribe_pane_raw` 现走 `register_pane_delta_channel`(cloudRemote) / `subscribe-pane`(bridge)
> 链路。返回 `head_seq` 的路径：改 `subscribe_pane_raw` 返回 `head_seq`，并让 bridge 的 `subscribe-pane`
> 把它带回 controller；或更简单——controller 订阅**成功后立即**调 `get_pane_scrollback_tail(pane, ~1.5屏)`
> 取 `{start_seq, head_seq, at_oldest}`，用 tail 作首屏、`start_seq` 作向上游标。tail 与 live 的 seam 用
> `head_seq` 界定（见上）。**首选后者**：改动更集中在控制端，host 仅新增 `head_seq` 字段。

### 分批向上翻页（懒加载）

控制端维护每 pane：`upCursor`(= 最旧已加载 seq)、`atOldest`、`fetching`。滚动到接近顶部（一屏内）触发
`get_pane_scrollback_before(pane, upCursor, BATCH)` → `prependScrollback` → `upCursor = chunk.start_seq`、
`atOldest = chunk.at_oldest`；`fetching` 去抖，`atOldest` 停。**完全复刻 `RidgePane.svelte::fetchOlderScrollback`**。

### 内存缓存 + 切换不重拉

- 路径 A：`paneScrollbackCache`(已存在)扩展存 `{tailBytes, upCursor, head_seq, atOldest}`；切回**先本地
  预绘**（零往返），再重订阅 live；切走时 unsubscribe live（允许「实时断开」）。切回补历史：新边界
  `head_seq'` → `before(head_seq', ~1.5屏)` 作 catch-up，用 `reconcileReplay` 决定 keep/repaint（已具备）。
- 路径 B：`manager.ts` 天然按 pane 保活内核 → 切 pane 本就不销毁不重拉；仅需白名单放行 + 首屏预算调小。

### host pane sink：真·多控制端

- **去掉 host 推 replay/RIS**：`handleSubscribePane` 不再调 `replay_pane_scrollback_raw`；历史全由控制端拉。
  → 消除「第 2 个 controller 冲掉第 1 个」。
- **live fan-out 引用计数**：`subscribe_pane_raw`/`unsubscribe_pane_raw` 按 pane 计数（N 个 controller
  订阅 → 1 条 fan-out，refcount=N；unsubscribe 递减，归零才拆）。→ 消除「co-viewer 断流」。
  live 广播本身正确（各 bridge 只 listen 自己订阅的 pane 事件）。
- **背压自愈改本地**：controller 侧 DataChannel drain 后，自己重取 `get_pane_scrollback_tail` RIS 重绘**自己**
  的内核（per-cid，不广播）。删 `resync_pane_raw`/desync 广播机制。

## 分阶段实施

**Phase 1（host，低风险高收益，先落地）**
- P1.1 白名单加 `get_pane_scrollback_tail`、`get_pane_scrollback_before`（`capability.rs` REMOTE_ALLOWLIST
  + `remoteAllowlist.ts` 镜像 + `remoteAllowlist.test.ts` 计数）。二者只读、不入 MUTATING_METHODS。
- P1.2 `ScrollbackChunk` 加 `head_seq: u64`（store 下一写入 seq），`get_pty_scrollback_tail`/`before` 一并回填。
- P1.3 `handleSubscribePane`（cloudHostBridge）不再调 `replay_pane_scrollback_raw`。
- **单独收益**：路径 B（桌面 in-browser）几乎零前端改动即变丝滑（白名单一放行，既有懒加载代码就通了），
  且不再有 host 256KiB dump；多控制端「冲屏」消失。

**Phase 2（控制端懒加载 + 缓存，主要针对路径 A 移动）**
- P2.1 `TerminalController` 暴露 `prependScrollback(bytes)`（转 `kernel.prepend_scrollback`）+ 滚顶检测。
- P2.2 `cloudRemote.ts`/`MainApp.svelte`：subscribe 用 `get_pane_scrollback_tail(~1.5屏)` 作首屏、滚顶
  `get_pane_scrollback_before` 翻页；`paneScrollbackCache` 存游标；切回本地预绘 + catch-up。
- P2.3 首屏预算：remote 用「约 1.5 屏」= `rows*cols*4` 上限夹在 [8KiB, 32KiB]（而非 256KiB）。

**Phase 3（host 多控制端隔离收尾）**
- P3.1 `cloud_pane.rs` fan-out 引用计数；删 replay/desync/resync 广播机制（历史/自愈已移控制端）。
- P3.2 背压自愈改控制端本地 RIS+tail 重绘。

**Phase 4（验证 + 收尾）**
- cargo check/clippy、vitest（含新纯逻辑 + 白名单计数）、`svelte-check` 全绿。
- `dev:cdp` 前端实测（弱网节流下首屏 ≤1.5 屏、滚顶分批、切 pane 零重拉）。
- 后端改动需 rebuild + 真机 e2e（多浏览器同 IP 并存、切 pane、滚顶历史）；由用户重启验证。

## 风险与边界

- **共享 tree**：本机 develop 多会话共用；按 hunk 只提交本设计相关改动（RidgePane 的
  `__GATE_DBG_TEMP__`、tauri.conf 版本号属他人改动，不碰）。见 [[feedback_shared_tree_git_amend]]。
- **白名单同步**：`capability.rs`（SSOT）与 `remoteAllowlist.ts`（镜像）必须逐字同步，计数测试兜底。
- **LAN 路径不回归**：Phase 1/3 只动 cloud sink 与 cloud bridge，不改 `server.rs` 的 RawBytes/LAN replay
  与 `RemotePtyEvent`（不给 live fan-out 加 per-chunk seq，改用 `head_seq` 边界，避免波及 LAN）。
- **旧控制端兼容**：host 停推 replay 后，未升级的旧控制端（仍依赖 host replay 首屏）会短暂空屏到首个
  live 输出。cloud host+controller 同版本分发，影响窗口小；如需可保留一个**极小**（~1.5 屏）per-cid replay
  兜底（不广播），作为过渡。
- **seam 竞态**：块级对齐下常态无重叠；忙碌 pane 订阅瞬间极端竞态 ≤1 块重复，自愈可接受。

## 提交粒度

- commit 1：设计文档（本文件）。
- commit 2：Phase 1（白名单 + head_seq + 去 host replay + 计数测试）。
- commit 3：Phase 2（控制端懒加载 + 缓存 + 纯逻辑测试）。
- commit 4：Phase 3（cloud_pane refcount + 去广播 + 背压本地化）。
