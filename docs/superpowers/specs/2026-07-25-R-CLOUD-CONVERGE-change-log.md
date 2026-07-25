# R-CLOUD-CONVERGE 变更/风险记录 —— cloud 首订阅 resync 全链路收敛（v0.1.2）

- **日期**: 2026-07-25
- **目标（用户 /goal）**: 按 NLM 工作流推进 cloud 首订阅自建帧缺口，**整个链路完成收敛**；Remote/Cloud
  有改动的都发布；提交并推送。
- **回退基线**: 本轮前 `HEAD`（v0.1.1，`eaaa2b0`）。若 P4/云路径回归，`git revert` 本轮 commit 即回 0.1.1 行为。
- **NLM 对抗评审**: 笔记本 `66919cb9-…`，conversation `a47d3199`（本文 §NLM 裁决逐条记）。

## 0. 一句话

host 新增 `get_pane_resync_frame` 出**一份完整** resync 帧（`RIS + 模式前导 + tail`，全经共享
SSOT `ridge_term::term::modes::build_resync_frame`），cloud 控制端**原样喂**；删前端两 invoke 自拼帧
（`get_pane_scrollback_tail` + `get_pane_resync_preamble` → `'\x1bc'+preamble+tail`）与已空转的
`get_pane_resync_preamble` 命令。**顺带修一个真 bug**（见 §2）。

## 1. 改动点（提交前扫）

| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/commands/terminal.rs` | **新增** `PaneResyncFrame` 结构 + `#[tauri::command] get_pane_resync_frame(pane_id, max_bytes)`；**删** `get_pane_resync_preamble`。 |
| `src-tauri/src/lib.rs` | invoke_handler：`get_pane_resync_preamble` → `get_pane_resync_frame`。 |
| `src-tauri/src/remote_host_impl.rs` | `CORE_MIGRATED_METHODS` 删 `get_pane_resync_preamble` 死条目（见 §2）。 |
| `packages/ridge-core/src/capability.rs` | `REMOTE_ALLOWLIST`（真能力门 SSOT）+`get_pane_resync_frame`。 |
| `packages/remote/src/shared/cloud/remoteAllowlist.ts` | TS 镜像同位 +`get_pane_resync_frame`（item-for-item 与 capability.rs 相等）。 |
| `packages/remote/src/shared/cloud/remoteAllowlist.test.ts` | 正向断言 `get_pane_resync_frame` 入门。 |
| `src/remote/lib/cloudRemote.ts` | `_subscribe` 首订阅：主 `get_pane_resync_frame`（一 invoke，原样喂）+ 版本偏斜兜底（`'\x1bc'+tail`）；加 `PaneResyncFrame` 类型；删 `get_pane_resync_preamble` 调用。 |
| `src/remote/lib/cloudRemote.test.ts` | mock 加 `get_pane_resync_frame`；改订阅断言；**补**版本偏斜兜底测。 |
| `packages/ridge-term/src/term/modes.rs` | **补**测试 `build_resync_frame_is_utf8_lossless_and_ssot_composed`（NLM 验收 g.1）。 |
| `package.json`/`tauri.conf.json`/`src-tauri/Cargo.toml`/`Cargo.lock` | 0.1.1 → **0.1.2**。 |

## 2. 顺带修的真 bug（dead cloud preamble）

上一版（v0.1.1）把 `get_pane_resync_preamble` **只**加进 `remote_host_impl.rs::CORE_MIGRATED_METHODS`
（"已迁 ridge-core 的**路由**表"，非能力门），**未**加进真正的能力门 `capability.rs::REMOTE_ALLOWLIST`
及其 TS 镜像 `remoteAllowlist.ts`。cloud 控制端 invoke 路径：`decideRemoteInvoke` →
`isRemoteAllowed(remoteAllowlist.ts)`；该命令不在表 → 被拒 `METHOD_NOT_FOUND` → 前端 `catch` 吞 →
`preamble=''`。**故 v0.1.1 "修公网手机 TUI 鼠标" 于 cloud 路径实为空转，鼠标仍失灵。** 本轮
`get_pane_resync_frame` 入**真门** → 前导真正随首帧下发 → cloud 路径鼠标真修复。
（教训：多份 allowlist 须同步；路由表 ≠ 能力门，勿混。）

## 3. NLM 对抗评审裁决（§④双门，逐条）

| 点 | NLM 主张 | 裁决 | 理由 |
| --- | --- | --- | --- |
| a `from_utf8_lossy` 破帧 | 高危，须改二进制 | **驳** | frame=ASCII(RIS)+ASCII(前导)+UTF-8 安全的 `ScrollbackChunk.bytes`，合法 UTF-8 → 无损（`Cow::Borrowed`）。**且现云路径本就串传**（JSON-RPC 无二进制），非新增风险。补无损断言测（g.1）即可。 |
| b 版本偏斜 | 不可接受，须能力协商 | **采纳** | 项目锁定不变量「两条独立版本线 + 能力先协商、未宣告显式拒非静默分叉」。云 PWA 独立发布线可新于旧桌面 host（host 侧 `isRemoteAllowed` 拒新命令）→ 留**极简兜底**（主 host 帧；退 `'\x1bc'+tail` 无前导 = 现发行为，非回归）。 |
| c 漏 rdg 调用方 | 恐回归 rdg | **驳** | grep `packages/ridge-cli` 零命中：rdg 直用 Rust SSOT（`build_resync_frame`/`ModeTracker`），从不调此 Tauri 命令。 |
| d 概念分层 | 可控（须返字节流非业务对象） | **采纳原设计** | 帧构建本在 ridge-term 引擎 SSOT；命令只读状态+调之，返 `{frame,seq…}` 非业务对象。 |
| e `head_seq` | P0 必加（R-INCR） | **采纳** | `ScrollbackChunk` 已算，一字段令新命令为旧 tail 完整超集 + R-INCR live/history 边界所需，非臆想。 |
| f 并删 `get_pane_scrollback_tail` / 收敛桌面 RidgePane 首屏 | 更狠减法 | **缓（记下轮）** | 越本轮范围（用户=云自建帧）。桌面 RidgePane 首屏 mount 亦经 `get_pane_scrollback_tail` 无前导 seed → 重挂时若 TUI 一次性开启已滑出，**桌面本地 pane 亦可能鼠标失灵**——真隐患，列下轮 **R-DESKTOP-RESYNC**。本轮不动桌面主路径（高影响，独立提交）。 |
| g 验收信号 | 补数据/负向/协议断言；虑分片背压 | **采纳（除背压）** | 加：modes.rs 无损+组成断言、remoteAllowlist.test 正向门断言、cloudRemote 原样喂+兜底测、`head_seq` 存在。**背压驳**：帧 ≤ `REMOTE_INITIAL_SCROLLBACK_BYTES`(16KiB) = 与现 tail 同尺寸，无新拥塞。 |

## 4. 风险点（P4 若返工/本轮若回归，快速定位）

- **R-C1（版本偏斜兜底）**: 旧桌面 host + 新云 PWA → 主命令被拒 → 走 `'\x1bc'+tail` 兜底（无前导）。
  **可接受退化**（历史仍绘，仅前导缺失 = v0.1.1 现状）；非黑屏、非丢数据。测：cloudRemote.test.ts
  「version-skew」用例。
- **R-C2（串传无损依赖）**: `get_pane_resync_frame.frame` 为 String，依赖「帧全 UTF-8」不变量。若未来让
  非 UTF-8 原始字节进帧 → `from_utf8_lossy` 出 `U+FFFD` 污染镜像。护栏：modes.rs
  `build_resync_frame_is_utf8_lossless_and_ssot_composed`。
- **R-C3（三份 allowlist 同步）**: `capability.rs`(真门 SSOT) ↔ `remoteAllowlist.ts`(TS 镜像) 须
  item-for-item 相等（`remoteAllowlist.test.ts` mirror-integrity 守卫）；`remote_host_impl.rs::CORE_MIGRATED_METHODS`
  仅路由、非门。**勿再把远程命令误加进 CORE_MIGRATED_METHODS 当放行**（本轮所修 bug 之根）。

## 5. 验收（确定性信号）

- `cargo check -p ridge -p ridge-core` → exit 0（含新命令 + allowlist 增项 + 删旧命令）。✅ 本机验。
- `cargo test -p ridge-core -p ridge-remote -p ridge-term` → 全绿；新 `build_resync_frame_is_utf8_lossless_and_ssot_composed` ok。✅ 本机验。
- **TS 门禁（CI）**: `remoteAllowlist.test.ts`（mirror-integrity item-for-item + 正向门断言）、
  `cloudRemote.test.ts`（订阅走 `get_pane_resync_frame` 原样喂 + 版本偏斜兜底）。本机 node_modules 残缺
  （缺 picocolors），vitest/svelte-check 不可跑 → **交 CI**（release.yml build 前的测试门待 R-TESTGATE 补强）。
- `grep get_pane_resync_preamble` 全仓仅余文档（P4 change-log 历史记述）→ 命令彻底移除。

## 6. 遗留（下轮）

- **R-DESKTOP-RESYNC（新，见 §3f）**: 桌面 `RidgePane.svelte` 首屏 mount 亦收敛到 `get_pane_resync_frame`
  （消「桌面本地 pane 重挂鼠标失灵」隐患）；随后可评估删 `get_pane_scrollback_tail`（如无其他调用）。
- **R-TESTGATE（P1，已在需求稿）**: 本轮 TS 断言无法本机跑、只能靠 CI，正是需要「发布前测试门」的又一实证。
