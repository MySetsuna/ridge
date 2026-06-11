# 设计:agent team(tmux 模式)宿主 pane 保护 —— 修复"启动 agent team 即崩宿主会话"

> 日期:2026-06-11 · 范围:`src-tauri`(Ridge teammate tmux 兼容层)· 类型:回归修复
> 触发:用户报告"启动 agent team,会话所在 pane 第一次直接退出会话,第二次直接关闭所在 pane"。

## 1. 现象

在 Ridge 终端 pane 里跑的 Claude Code(`teammateMode: tmux`)一旦 spawn teammate,
**宿主会话(parent claude)所在的 pane 被摧毁** —— 表现为会话退出 / pane 关闭。
大重构前可用;重构后回归。

## 2. 架构背景

Claude Code 在 `teammateMode=tmux` 下把"开一个 teammate"想成 `tmux split-window` +
`send-keys 'claude --agent-id ...'`。Ridge 用 shim 二进制(`src-tauri/src/bin/tmux.rs`)
拦截这些 `tmux` 子命令,转成本地 HTTP(`/api/v1/*`),由 `src-tauri/src/teammate/server.rs`
在真实 Ridge pane_tree 上分屏 + 起 PTY。`%N` pane 目标 = `pane_tree.get_all_leaves()` 的索引,
**索引 0 = 第一个叶子 = 宿主原始 pane(parent claude 所在)**。

## 3. 根因(代码 + shim 实录双重确认)

`%TEMP%/tmux-shim.log` 记录的 claude-sdk 实际命令序列(socket=default 即 GUI-bridge):

```
split-window -t 0 -h -l 70% -P -F #{pane_id}   → POST /split-window → new_pane_index=0   ⚠
select-pane/set-option ... 给 %0 设标题/边框
send-keys -t %0 "... claude.exe --agent-id <name> ..."  → POST /spawn-process → success
kill-pane -t %0                                 → POST /kill-pane                          ⚠
```

历史(可用)日志同序列返回 `new_pane_index=1`,且 spawn 后**无**立即 kill-pane
(kill-pane(1) 只在 agent 数十分钟后退出时触发)。**差异 = 现在 split 返回 0、且 spawn 后立即 kill(0)。**

两个缺陷叠加,把整个 teammate spawn 漏斗到宿主 pane:

### 缺陷①:idle-reuse 把宿主 pane 当"空闲可复用 pane"
`teammate/server.rs::find_idle_pane_index` 返回"第一个非 Busy 的 Terminal 叶子"。
宿主 pane 是 Terminal、且**从未被 `register_teammate_agent` 标记 Busy** → 命中为"空闲",返回 idx 0。
`route_split`(`auto_place` 时)据此复用宿主、返回 `new_pane_index=0`、置 `teammate_tmux_pane_cursor=0`。

### 缺陷②:spawn-process / kill-pane 落到宿主、无守卫
- `route_spawn_process` 对 `pane_idx`(cursor=0=宿主)调 `ensure_pane_pty_workspace` →
  `teardown_pane_pty_if_present` **替换宿主 PTY**(`teammate_replace_pty`)→ parent claude 当场死。
- `route_kill_pane` 对 `pane_index:0` 解析出宿主 Uuid,直接 `kill_pty_if_present(...,true)` +
  `pane_tree.close()`。所谓"T5 fail-closed"**只校验工作区,没校验目标 pane 是不是宿主**。
- 配套回归:shim `cmd_kill_pane` 已从历史的 **no-op** 改为**真 POST `/api/v1/kill-pane`**
  (注释:"Without this the pane lingers as a zombie")。

> claude-sdk 的 `kill-pane -t %0` 本是"teammate 退出 → 杀其 pane"的正常清理;历史上 teammate
> 在 pane 1,kill(1) 无害。回归后 teammate 被塞进 pane 0(宿主),触发 kill(0) → 杀宿主。

## 4. 修复:teammate-owned-panes 唯一可信边界

引入每工作区的 `teammate_owned_panes: HashSet<Uuid>` —— **只有 Ridge 亲手为 teammate 建的 pane
进集合**;宿主/用户 pane 永不进集合。三处守卫 + 两处维护:

| # | 位置 | 改动 |
|---|---|---|
| 维护 | `commands/pane.rs::teammate_split_pane` | split 出新 pane 即 `insert(new_id)`(覆盖 split 与 new-window 回退两条建 pane 路径) |
| 维护 | `commands/pane.rs::close_pane` / `server.rs::route_kill_pane` | 关/杀 pane 时 `remove` |
| 守卫① | `server.rs::find_idle_pane_index` | **只返回 owned 内的 pane** → 宿主永不被复用 → 第一个 teammate 恢复新建(idx≥1) |
| 守卫② | `server.rs::route_spawn_process` | 目标 pid 非 owned → 拒绝(防 cursor=0/默认误伤,杜绝替换宿主 PTY) |
| 守卫③ | `server.rs::route_kill_pane` | 目标 pid 非 owned → 静默 OK no-op(claude-sdk teardown 流不报错,宿主免疫) |

### 不变量
- 宿主 pane 与任意用户手开 pane 永不属于 `teammate_owned_panes` → 三条破坏性路径(复用/spawn/kill)结构性碰不到它。
- 正常 harness 流不受影响:bare split → 建 owned 新 pane(idx≥1)→ cursor=新 idx → spawn-process 命中 owned → 起 teammate;agent 退出 → pane 回 Idle 仍 owned → 下个 teammate 复用。

## 5. 回归点(供"找回大重构前可用版本"对照)
- `find_idle_pane_index` 丢失了"排除宿主 / 只复用 teammate pane"的语义(大重构,squash 提交,难精确定位)。
- shim `cmd_kill_pane`:no-op → 真 POST `/api/v1/kill-pane`(为修"teammate 退出后留空壳 pane")。

## 6. 验收
- 静态:`cargo check --lib --bin tmux` 干净;`cargo test --bin tmux` shim 逻辑绿。
- 运行(需 rebuild+reinstall,会重启宿主会话,由用户手动跑):
  `~/.claude/agent-team/ridge-tmux-acceptance.sh` → `FAIL=0 SKIP=0`;
  然后在 Ridge pane 里启 Claude Code 真 spawn 一个 teammate:**宿主 pane 存活**,
  teammate 落在独立新 pane(idx≥1),agent 退出后只关 teammate pane。
- Windows lib-exe 限制:后端逻辑不能作为独立测试 exe 运行,靠 cargo check + 上述运行验收收口。

## 7. Runtime 验收结果(2026-06-11,dev:cdp 独立实例)

`pnpm tauri:dev:cdp` 起独立 debug 实例(CDP :9222、teammate server :56884、`RIDGE_DISABLE_SINGLE_INSTANCE=1`),
经 `__TAURI__.invoke('write_to_pty', ...)` 在宿主 pane 注入**与崩溃 shim 日志逐字相同**的 claude-sdk 序列:

- `cargo build` **Finished**(3m05s)→ 修复编译通过(真构建,非仅 rust-analyzer)。
- `tmux split-window -t 0 -h -l 70% -P -F '#{pane_id}'` → **`SPLIT_RETURNED=%1`**(新建独立 pane,不复用宿主);
  shim 日志 dev 实例 `new_pane_index=1` ⟷ 安装版旧代码 `new_pane_index=0`,直接对照。
- `tmux kill-pane -t %0` → 脚本继续输出 **`HOST_SURVIVED_AFTER_KILL_PANE_0`**;后端 `get_pane_layout` 确认
  宿主 leaf `95b7caaa…` 存活、新 teammate leaf 创建。→ 缺陷①②均已修复,宿主免疫。
- 真 `claude` 进程级复现因 **claude native binary 未装在 dev pane 的 node(nvm4w)环境**而无法启动
  (`rc=127 command not found` / `native binary not installed`)——属环境/安装问题,与本修复无关;
  且 exact-replay 的字节与 claude-sdk 发出的完全一致,已构成忠实验证。

## 8. 附加改动:tmux 拉起但未运行 agent 的 pane 不打 agent 标(用户需求 2026-06-11)

**问题**:`route_split` 非复用路径对**裸 split**(无 agent / 无结构化命令)标 `PaneState::Starting`,
`spawn_starting_watchdog` 再据"有活 shell 子进程"把它**提升为 `Busy`** → 纯 tmux shell pane 被误打 agent 标
(该看门狗注释自己就承认这个误标"触发面极窄...接受")。

**修复**(`teammate/server.rs`,3 处):
- 裸 split **不写任何 `teammate_pane_states`**(仅 `is_agent`/结构化 program 才标 `Busy`)→ 与普通用户 pane 同款、无 badge。
- 移除 `spawn_starting_watchdog` 调用,并**彻底删除**该看门狗函数 + `STARTING_WATCHDOG_GRACE` 常量
  —— 它是"有活子进程≈agent→标 Busy"的启发式,对任何非 agent shell 都会误判;新设计下真 agent 在源头
  (spawn-process(is_agent)/结构化 split)直接标 Busy,不会再有"卡 Starting"需救援的场景,故该看门狗
  守的是一个已不存在的情况,删之。(`PaneState::Starting` enum 变体保留:`pane.rs` 序列化处仍 match。)
- harness 主路径不受影响:`split→spawn-process(is_agent)` 由 spawn-process 在 agent 真正落入时标 `Busy`。
- 无需前端改动:无 `teammate_pane_states` 条目 → 布局 `agent_state` 缺省 → 前端不渲染 badge(与宿主 pane 一致)。
