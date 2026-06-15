# Cloud 弱网韧性 实现级设计

- 日期：2026-06-11
- 作者：weaknet-reviewer（plan 阶段，**只设计不改码**）
- 依据：`C:\code\wind\.agent-team\findings-weaknet.md`（P0×1 / P1×2 / P2×5）
- 范围：wind 仓库 cloud 远控链路（`src/lib/remote/cloud/*`、`src/lib/transport/remote/*`、`src-tauri/src/commands/cloud_pane.rs`）+ ridge-cloud 信令 relay（`src/ws/*`）+ LAN host 写路径（`src-tauri/src/remote/server.rs`）

---

## 0. 设计总纲：复用 LAN，不另起炉灶

审核的核心结论是「LAN 韧性扎实、cloud 系统性缺失」。本设计的第一原则因此是 **cloud 复用 LAN 已验证的机制与 L2 共享层，而非另写一套**。三处可直接复用的既有资产：

1. **L2 重连重同步是 transport 无关的，且已经在 LAN 上跑通**
   `bridge.attach()` 注册了 `rpc.onReconnected(() => 重订阅所有 subscribedPanes)`（`bridge.ts:77-83`）；`RpcClient.handleStateChange` 在「非 connected → connected」边沿运行这些 hook 并重发 `$/hello`（`rpcClient.ts:348-358`），在「connected → error/disconnected」边沿 reject 所有在途请求（`rpcClient.ts:338-343`）。
   LAN 侧 `RemoteConnection` 重连后 emit `connected` → `LanWsAdapter` 透传状态（`lanWsAdapter.ts:117-119`）→ 触发上述 hook。**cloud 侧只要让 provider 在断线后真正重连并重新 `setState('connected')`，这套「死代码」就被激活——无需新写重同步逻辑。**

2. **LAN 的「有界队列满 → 丢帧置 `desync` → RIS + 64KiB scrollback 重同步（限频 1/s）」是输出洪峰恢复的范本**
   生产端 `lib.rs:304-316`（满即 `desync.store(true)`）；恢复端 `server.rs:2150-2168`（读 `desync` → 发 `\x1bc` + `get_recent_scrollback_for(ws,pane,65536)`）。cloud 已复用生产端（`cloud_pane.rs:48-79` 用同款 512 队列、第 56 行建了 `desync`），**只差把恢复端搬过来**。

3. **LAN `RemoteConnection` 的重连/心跳参数曲线**（`wsRemote.ts:31-37`）经过移动端实战，cloud 直接对齐同一组常量，避免两端参数漂移。

> 命名约定：下文 cloud 新增的重连/心跳/背压常量，一律取与 LAN 同名同值（见各节「与 LAN 对齐」）。

---

## 1. P0 — cloud 自动重连 + 重连后会话重同步

### 1.1 问题回顾
`ControllerCloudProvider` / `RidgeCloudHost` 无任何重连路径：RTC `failed/closed`、`dc.onclose`、信令 `ws.onclose` 一律走 `fail()` → `setState('error')` 终态（`controllerCloudProvider.ts:172-179/194-196/354-359/128-131`；`ridgeCloudProvider.ts:470-476`）。L2 已写好的重同步因 transport 永不回 `connected` 而成死代码。

### 1.2 设计：在 provider 内置「断线 → 退避重连 → 重新 connected」状态机（controller 侧）

让 `ControllerCloudProvider` 的状态生命周期**镜像 `RemoteConnection`**：把当前「断 → error 终态」改成「断 → disconnected → 退避计时 → 重跑 connect 流程 → handshaking → connected」。

关键点（按代码位置）：

- **触发源归一**（`controllerCloudProvider.ts:172-179/194-196`）：`pc.onconnectionstatechange === 'failed'` 与 `dc.onclose`（非主动 close）不再调 `fail()`，改调新增的 `scheduleReconnect()`；先 `setState('disconnected')`（驱动 L2 reject 在途），再排重连。`'disconnected'`（ICE 抖动）维持现状不立即动作，但**加一个 disconnected 看门狗**：若 8s 内未自愈回 `connected`，升级为重连。
- **ICE 优先免重建**：重连第一跳先试 `pc.restartIce()` + 重新 `createOffer({iceRestart:true})` 经信令发出（需信令 WS 在线，见下）；仅当 `pc` 已 `failed/closed` 或 restartIce 超时（如 6s 无 `connected`）才整体 teardown + 重建 PC/DC/E2EE。
- **信令 WS 独立重连**（`controllerCloudProvider.ts:354-359`）：`ws.onclose`（非主动）→ 重连信令通道（指数退避），与 RTC 重连解耦——信令断不等于媒体断，但媒体要恢复/续 ICE 必须先把信令拉回。
- **E2EE 会话重置**：整体重建分支里 `resetBinding()` + 新 `generateEphemeralKeyPair()`（握手天然重跑，`startE2eeHandshake` 已有）。counter 从 0 重起（新 `E2eeSession`），不触发 `e2ee.ts:212` 的单调校验问题。
- **重新 connected 即自动重同步**：重建/恢复后 `setState('connected')` → adapter `handleProviderState`（`cloudWebrtcAdapter.ts:208-219`）→ `RpcClient.handleStateChange` 跑 `onReconnected` hook（`bridge.ts:78` 重订阅所有 pane）+ 重发 `$/hello`（`rpcClient.ts:349-351`）。**host 端 `subscribe-pane` 会从 scrollback 重放当前屏**（host 的 pane 源接 server.rs fan-out），所以重连是「状态续传」而非黑屏重来。

设计草图（**示意，非改动**）：
```ts
// ControllerCloudProvider 新增字段
private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
private reconnectAttempts = 0;
private intentionalClose = false;       // disconnect() 置 true，抑制重连
private disconnectedWatchdog: ... | null = null;

private scheduleReconnect(): void {
  if (this.intentionalClose || this.reconnectTimer) return;
  const n = this.reconnectAttempts++;
  const base = Math.min(RECONNECT_BASE_MS * 2 ** n, RECONNECT_MAX_MS); // 1s→15s
  const delay = Math.round(base + base * 0.3 * Math.random());          // ±30% 抖动
  this.setState('disconnected');                                        // L2 reject 在途
  this.reconnectTimer = setTimeout(() => {
    this.reconnectTimer = null;
    void this.reconnect();   // 先 restartIce；不行则 teardown+重建，最终 setState('connected')
  }, delay);
}
// connect 成功后 reconnectAttempts=0（对齐 wsRemote.ts:258）
```

### 1.3 host 侧（`RidgeCloudHost`）
host 是「常驻在线」方，断的主要是**信令 WS**（`ridgeCloudProvider.ts:470-476`）。设计：`ws.onclose`（非主动）→ 不进 `error` 终态，改 `setHostState('connecting')` + 退避重连 `openSignaling(deviceId)`；重连成功收到 `welcome` 即恢复 `online`。已建立的 per-controller RTC **不拆**（relay 下线不影响 P2P），重连只为恢复「接纳新 controller / 续 ICE / 重协商」。每条 `CloudHostBridge` 已有 `reset()`，仅在对应 controller RTC 真断时调。

### 1.4 参数建议（与 LAN 同名同值，`wsRemote.ts:36-37`）
| 常量 | 值 | 含义 |
|---|---|---|
| `RECONNECT_BASE_MS` | `1_000` | 退避基数 |
| `RECONNECT_MAX_MS` | `15_000` | 退避上限 |
| 抖动 | `±30%` | 防重连风暴同步 |
| ICE restart 判失败 | `6_000ms` 无 connected | 超时则整体重建 |
| disconnected 看门狗 | `8_000ms` | ICE 抖动自愈宽限 |

### 1.5 改动文件清单
- `src/lib/remote/cloud/controllerCloudProvider.ts`
  - L172-179：`onconnectionstatechange` failed/disconnected 分支改接 `scheduleReconnect`/看门狗。
  - L194-196：`dc.onclose` 非主动 → `scheduleReconnect`。
  - L354-359：信令 `ws.onclose` 非主动 → 信令重连。
  - L443-470：`disconnect()` 置 `intentionalClose=true` + 清 `reconnectTimer`/看门狗。
  - 新增 `scheduleReconnect()` / `reconnect()` / 信令重连方法 + 上述字段。
- `src/lib/remote/cloud/ridgeCloudProvider.ts`
  - L470-476：host 信令 `ws.onclose` → 退避重连 `openSignaling`（L450-477 复用）。
  - 新增重连字段/方法 + `goOffline()`（L205-216）置 `intentionalClose`。
- **无需改** `rpcClient.ts` / `bridge.ts` / `cloudWebrtcAdapter.ts`：重同步链路已就绪，复用即可（这是与 LAN 对齐的最大收益）。

### 1.6 与 LAN 对齐说明
直接复刻 `RemoteConnection` 的 `_scheduleReconnect`（`wsRemote.ts:362-373`）退避曲线与 `_intentionalClose` 抑制逻辑；复用 `RemoteConnection` 已验证：cloud provider 不写自己的重同步，借 L2 `onReconnected`（与 LAN 同一段 `bridge.ts:77-83`）。

---

## 2. P1 — 输出洪峰背压 + 丢帧重同步

### 2.1 问题回顾
- host 发送 `dc.send()` 不查 `bufferedAmount`（`ridgeCloudProvider.ts:422-428`、`controllerCloudProvider.ts:324-329`），弱上行下 SCTP 缓冲无界增长，溢出后 `dc.send` 抛错被 `pushPaneOutput` 吞掉「dropped」（`cloudHostBridge.ts:537-549`），无重同步 → 永久花屏。
- `cloud_pane.rs:48-79` 复用 512 队列、第 56 行建了 `desync` **却从不读取**，连 LAN 那套 mpsc-满恢复都没接上。

### 2.2 设计：两层背压，统一复用 LAN 的「RIS + scrollback」恢复原语

**第①层（Rust mpsc 满，直接镜像 server.rs）**：`cloud_pane.rs` 转发任务在转发裸字节前，复刻 `server.rs:2150-2168` 的 desync 检查块——读 `sub.desync`，若置位则限频（≥1s）发一帧 `RIS(\x1bc) + get_recent_scrollback_for(ws,pane,65536)` 经 `pane-raw-{paneId}` 推给 WebView，再发当前帧；消费 desync 标志（`store(false)`）。这恢复了「Tauri emit/下游慢 → mpsc 满丢帧」的空洞。

**第②层（DataChannel 拥塞，cloud 特有，真正的主瓶颈）**：在 `CloudHostBridge.pushPaneOutput`（或 provider `rawSend`）加 `bufferedAmount` 水位流控：
- `bufferedAmount > HIGH_WATERMARK` → **丢弃当前 pane 帧**（不再无界缓冲），置 JS 侧 `paneDesync=true`；
- 监听 `dc.bufferedamountlow`（`dc.bufferedAmountLowThreshold = LOW_WATERMARK`）→ 缓冲回落后，若 `paneDesync`，向 host 请求一次该 pane 的 RIS+scrollback 重放。

cloud 的 scrollback 在 **host 的 Rust 端**，故第②层的「请求重放」最简实现：bridge 发一个 `resync-pane`（notification，复用 `subscribe-pane` 同款路由）→ host 的 pane 源重新发 RIS+scrollback。若不想新增消息，可直接复用「unsubscribe + 重新 subscribe-pane」触发 host 重放当前屏（host `subscribe-pane` 本就从 scrollback 起播）。

> 端到端效果：DataChannel 拥塞 → 丢帧（有界，不 OOM）→ 缓冲回落 → host 重放当前屏 → controller vte 自愈。与 LAN「队列满丢帧 + 下一帧 RIS+scrollback」语义一致。

设计草图（**示意**，host bridge 侧）：
```ts
pushPaneOutput(paneId, raw) {
  if (this.rejected || !this.verified) return;
  if (this.dcBufferedAmount() > HIGH_WATERMARK) {  // 背压：丢帧而非无界缓冲
    this.paneDesync.add(paneId);
    return;
  }
  try { this.sendFrame(encodePaneFrame(paneId, raw)); }
  catch { this.paneDesync.add(paneId); }           // send 抛错也置 desync
}
// dc.onbufferedamountlow → 对每个 paneDesync 的 pane 触发 host 重放（resync-pane / re-subscribe）
```

### 2.3 参数建议
| 参数 | 值 | 依据 |
|---|---|---|
| `HIGH_WATERMARK`（bufferedAmount 上水位） | `8 MiB` | 远低于 libwebrtc 16MB 硬上限，留 8MB 余量给在途帧 |
| `LOW_WATERMARK`（`bufferedAmountLowThreshold`） | `1 MiB` | 回落到此恢复泵/触发重放 |
| 重同步限频 | `≥ 1s`（`RESYNC_MIN_INTERVAL`，与 server.rs:1296 同名同值） | 防拥塞放大反馈环 |
| scrollback 回放量 | `65536` 字节（与 server.rs:2158 一致） | 两端对齐 |
| 队列容量 | `512`（`RAW_CHAN_CAP`，cloud_pane.rs:27 已是） | 与 LAN 一致 |

### 2.4 改动文件清单
- `src-tauri/src/commands/cloud_pane.rs`：L62-79 转发任务循环内加 desync 检查 + RIS+scrollback 重放块（移植 `server.rs:2150-2168`）；需访问 `state.get_recent_scrollback_for`（已有）与 `last_resync` 限频计时。
- `src/lib/remote/cloud/cloudHostBridge.ts`：L537-549 `pushPaneOutput` 加 bufferedAmount 水位判断 + `paneDesync` 集 + 暴露 `dcBufferedAmount()`/`onBufferedAmountLow` 注入点。
- `src/lib/remote/cloud/ridgeCloudProvider.ts`：L422-428 `rawSend` 暴露 `bufferedAmount` 读取 + 给 bridge 接 `bufferedamountlow` 事件（DC 在 `attachDataChannel` L286-299 设 `bufferedAmountLowThreshold`）。
- （可选）host 侧 pane 源新增 `resync-pane` 处理或复用 re-subscribe 触发重放。

### 2.5 与 LAN 对齐说明
第①层逐行移植 `server.rs:2150-2168`；常量名/值（`RESYNC_MIN_INTERVAL`、65536）与 LAN 同。第②层是 cloud 特有（LAN 走 TCP，无 DataChannel buffer 概念），但**恢复原语仍是同一套 RIS+scrollback**，只是触发条件从「mpsc 满」扩展到「DataChannel 水位」。

---

## 3. P1 — 信令 WS 心跳 + relay 空闲超时

### 3.1 问题回顾
relay 读循环忽略 Ping/Pong、不主动发 ping、无空闲计时（`handler.rs:341`）；`/ws` 在 20s `TimeoutLayer` 外层（`router.rs:118-122,147`）故既不会被误杀也完全无 keepalive。公网 NAT 静默掐断空闲信令 WS 后，host 无察觉（无心跳）+ 无重连（P0）→ 假在线。

### 3.2 设计：relay 端主动 Ping + 空闲判死（首选），客户端靠 onclose 触发重连

选 **relay 驱动** 而非应用层双向 ping，理由：浏览器 WebSocket API 不暴露发送原始 Ping/收 Pong 事件，但**会在协议层自动 Pong** 回应服务器 Ping——故 relay 发 `Message::Ping`、浏览器自动回 Pong，零客户端代码即可维持 NAT 映射并探活。

relay 改造（`handler.rs` 的 `run_connection` 写循环 / select）：
- 加一个 `tokio::time::interval(PING_INTERVAL)`，每 tick 发 `Message::Ping(vec![])`；
- 维护 `last_pong: Instant`，读循环里 `Ok(Message::Pong(_))` 分支更新它（`handler.rs:341` 当前是空 body，改为记录）；任何入站帧也可视作存活；
- 若 `now - last_pong > IDLE_DEAD`，`break` 关闭连接 → `WsPermit` Drop 归还 `per_user_max` 名额（`limits.rs`），并让 host/controller 收到 `onclose` → 触发各自重连（P0）。

host/controller 侧**不需要**新增应用层 ping：依赖 relay 关闭后的 `ws.onclose` → P0 的信令重连即可。若要 host 更快主动探测（缩短假在线窗口），可选地加客户端应用层 `{t:'ping'}`，但需 relay 路由支持——列为可选增强，非必须。

### 3.3 参数建议（高 RTT 不误判）
| 参数 | 值 | 说明 |
|---|---|---|
| `PING_INTERVAL` | `25s` | < 多数 NAT 30–120s 空闲回收窗口下沿，维持映射 |
| `IDLE_DEAD`（判死） | `70s`（≈ 2–3 个漏 pong） | RTT 即便 500ms+ 也远小于 25s 间隔，判死阈值 70s 给足冗余，**绝不误判高延迟为掉线** |
| 名额回收 | 判死即 Drop WsPermit | 防半开连接长期占 `per_user_max` |

> 误判分析：判死靠「连续 ~3 个 25s 周期无任何入站」，与单帧 RTT 无关——RTT 500ms 仅让 Pong 晚 0.5s 到达，对 70s 阈值毫无影响。

### 3.4 改动文件清单
- `ridge-cloud/src/ws/handler.rs`：`run_connection`（L189-393）的收发循环改为 `tokio::select!` 含 `ping_interval.tick()` 分支发 `Message::Ping`；L341 `Pong` 分支记 `last_pong`；新增 idle 判死 break。
- （可选）`ridge-cloud/src/ws/messages.rs`：若走应用层 ping 才需加 `{t:'ping'/'pong'}`，首选方案不需要。
- 客户端无需改（靠 onclose + P0 重连）。

### 3.5 与 LAN 对齐说明
LAN 是客户端应用层 `{type:'ping'}`/`{type:'pong'}`（`wsRemote.ts:394-415`，间隔 15s、判死 10s），因为 LAN host（axum split sink）侧同样不便发原始 ping。cloud 反过来用 relay 发协议级 Ping（浏览器自动 Pong），是更省客户端代码的等价探活。判死阈值取比 LAN 宽（70s vs 10s）是因为信令 WS 空闲是常态、且断了有 P0 重连兜底，宽容度换取零误判。

---

## 4. P2 — 传输压缩（两端）

### 4.1 问题回顾
全仓库无 permessage-deflate / 应用层压缩；终端输出（重复 ANSI、日志、进度条）通常可压 5–10×，弱/计量链路浪费带宽并放大洪峰。

### 4.2 设计
- **LAN**：开启 WebSocket permessage-deflate。axum 底层 tokio-tungstenite 支持；在 `server.rs:1262` `handle_ws` 的 socket 配置处启用压缩（`WebSocketUpgrade` 协商扩展）。客户端浏览器 WS 自动协商，无需改前端。零协议改动、对原始 ANSI 直接见效。
- **cloud**：E2EE 后密文不可压，必须在**加密前**对 `0x10` pane 帧压缩。设计：在 `cloudMux.ts` 增 1 个压缩变体通道 tag（如 `0x13 PANE_RAW_DEFLATE`），host 编码端对 raw 字节先 deflate 再 seal，controller demux 端识别 `0x13` 先 open 再 inflate。控制帧（0x11/0x12）量小不压。需双端同步实现（host `cloudHostBridge.ts` 编码 + controller `cloudWebrtcAdapter.ts` demux）。

### 4.3 参数/算法建议
- 压缩算法：LAN 用 WS 内建 deflate；cloud 用 deflate 或 zstd（@noble 之外需引库，评估包体）。
- 阈值：小帧（如 < 256B）不压（压缩头反增大）；仅对 pane 帧压。
- 与 P2-2 合帧（§5）协同：先合帧再压缩，压缩率更高。

### 4.4 改动文件清单
- `src-tauri/src/remote/server.rs`：L1262 区域启用 permessage-deflate。
- `src/lib/transport/remote/cloudMux.ts`：L30-45 增 `PANE_RAW_DEFLATE=0x13` + 编/解码。
- `src/lib/remote/cloud/cloudHostBridge.ts`：`pushPaneOutput` 编码前压缩。
- `src/lib/transport/remote/cloudWebrtcAdapter.ts`：`handleInboundFrame` demux 解压。

### 4.5 与 LAN 对齐说明
LAN 借 WS 标准扩展「免费」获得压缩；cloud 因 E2EE 必须自管「加密前压缩」，但通道 tag 约定沿用 `cloudMux` 既有 1 字节前缀框架，保持与现有 0x10/0x11/0x12 同构。

---

## 5. P2 — LAN host 写路径半开死连超时（一并列入）

### 5.1 问题回顾
`server.rs` 的 `ws_tx.send(...).await`（L2164/2172/2315 等）无写超时；客户端半开时阻塞整个 per-connection select 循环（含 1s health_interval / admin 断开），直到 OS TCP 超时（Windows 默认数分钟）。

### 5.2 设计
对所有 `ws_tx.send(...)` 套 `tokio::time::timeout`：
```rust
match tokio::time::timeout(WRITE_TIMEOUT, ws_tx.send(msg)).await {
    Ok(Ok(())) => {}
    Ok(Err(_)) | Err(_ /* elapsed */) => break,  // 写错误或超时 → 判客户端死亡，清理
}
```
超时即 `break` 走既有清理路径（L2349+ 注销 sub）。客户端心跳（`wsRemote.ts`）本就会让客户端侧自愈重连，本改动只是让 **host 侧**不再滞留卡死任务 + 512 缓冲。

### 5.3 参数建议
| 参数 | 值 | 说明 |
|---|---|---|
| `WRITE_TIMEOUT` | `10–15s` | > 正常弱网慢发，< OS TCP 判死；与客户端 `PONG_TIMEOUT_MS=10s` 量级对齐 |
| 备选 | 启用 TCP keepalive | 缩短内核判死，作为兜底 |

### 5.4 改动文件清单
- `src-tauri/src/remote/server.rs`：抽一个 `send_or_break!(ws_tx, msg, WRITE_TIMEOUT)` 宏/辅助，替换 select 循环内所有 `ws_tx.send(...).await`（RawBytes L2164/2172、Metadata/Resize、structural、ui_event、health-close 各处）。

### 5.5 与 LAN 对齐说明
超时量级与 LAN 客户端心跳判死（10s，`wsRemote.ts:32`）对齐：客户端 10s 判死会主动重连开新 socket，host 端 10–15s 写超时正好让旧 socket 同窗口回收，两端判死节奏一致。

---

## 6. 落地次序建议（每项可独立成 commit）
1. **P0 重连**（最高收益，激活已有重同步）—— controller 优先、host 次之。
2. **P1 背压+重同步**（第①层 cloud_pane.rs desync 移植先行，低风险；第②层 bufferedAmount 流控随后）。
3. **P1 relay 心跳/空闲超时**（ridge-cloud 独立改动，与 wind 解耦）。
4. **P2 写路径超时**（LAN host 小改，独立）。
5. **P2 压缩**（最后，体积/依赖评估后，LAN 先于 cloud）。

## 7. 验证要点（实现阶段写测试时参考，本轮不写测）
- 重连：断网/恢复后 pane 自动续播不黑屏；在途 invoke 被 reject 不悬挂；`$/hello` 重协商。
- 背压：`cat 大文件` over 限速链路，bufferedAmount 不越 16MB、不 OOM；丢帧后回落自愈不花屏。
- 心跳：模拟 NAT 空闲掐断（70s 无 pong）relay 关连接、归还名额；RTT 500ms 注入不误判。
- 写超时：host 对半开客户端在 WRITE_TIMEOUT 内回收，admin 断开及时生效。

## 8. 不在本设计范围
- E2EE 切 unordered DataChannel 的重放窗口（findings P2-5，前瞻项，当前 ordered 无 bug）。
- relay OUTBOUND_CAP 调参（findings P2-4，低概率）。
- host WebRTC 迁 Rust(webrtc-rs)（契约 §8 终态，跨期）。
