# 开放愿景清单 — Remote / multi-host / agent 监控 / mobile touch（iteration 19）

更新：2026-07-24  
库存：`2026-07-24-remote-multihost-agent-inventory.md`

| ID | 主题 | 状态 | 证据 |
| --- | --- | --- | --- |
| **R19-TOUCH-ALT** | 移动滑屏在 alt-screen 无 mouse 时转方向键 | **implemented** | `mobileTouchScroll.decideTouchScroll` + TerminalCanvas.touchWheel；测 `mobileTouchScroll.test.ts` |
| **R19-TOUCH-REL** | 触屏 release 与桌面 SGR btn=3 对齐 | **implemented** | `decideTouchMouseGesture('release')`；TerminalCanvas selection/tap |
| **R19-ORCH-REMOTE** | Remote Team 面板绑真实 orch health | **implemented** | allowlist `get_orchestration_health`；RemoteLink + cloud/ws；SidebarTeamRoster badges |
| **R19-ROSTER-SUS** | roster Suspended 状态可见 | **implemented** | SidebarTeamRoster `dot.suspended` + 角色文案 |
| **R19-DUAL-END** | dual-end 订阅/refcount/desync 主路径 | **implemented*** | *既有 cloud_pane refcount + owning sub_id + reconnect teardown；本弧补 orch 只读 admit；无新状态源 |
| **R19-HOSTS** | multi-host attach/list/fanout | **implemented*** | *既有 hosts 测；本弧无新缺口 |
| **R19-WS-PTY** | 完整出站 WS PTY 客户端 | **rejected** | 下一里程 / non-goal |
| **R19-AGENT-REDESIGN** | Agent Center 视觉大改 | **rejected** | 仅数据绑定与 dual-end 放行 |

**open 计数：0**

## 门禁

```
pnpm exec vitest run packages/remote/src/shared/terminal/mobileTouchScroll.test.ts packages/remote/src/shared/transport/capabilityContract.test.ts packages/remote/src/shared/cloud/remoteAllowlist.test.ts
cargo test -p ridge --lib hosts:: teammate::orch_health
```
