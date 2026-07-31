# REQ-RIDGE-KERNEL-DOMAIN-01 · 能力出口清单（本轮铺垫）

intake: `INTAKE-20260731-KERNEL-NEXT-01`  
日期: 2026-07-31  
原则: 权威实现须在内核进程可调用；桌面 `#[tauri::command]` 仅薄包装。

| 能力域 | 当前权威实现 | 桌面入口 | 无 GUI / rdg | 内核绑定状态 |
| --- | --- | --- | --- | --- |
| 工作区文件/目录 | `ridge_core::fs` + desktop fs 命令 | `commands/fs*` | `rdg kernel fs-list` → `/v1/domain/fs/list` | **已绑只读 list**（kernel 进程） |
| Agent profiles | builtin 表（kernel 内嵌 + 桌面 catalog） | settings / catalog | `rdg kernel agents` | **已绑 profiles 只读** |
| MCP 最小面 | kernel `/api/v1/mcp` | bridge 发现 kernel.json | `rdg kernel mcp-smoke` | **已绑** initialize+tools |
| Git / SCM | `ridge_core` git | `commands/git.rs` | 库可用；**未**挂 kernel 路由 | 待迁 |
| 远端 Host / LAN | desktop + rdg 各 host | Tauri / rdg dashboard | 分叉 | 待迁 |
| Agent 花名册/编组 runtime | teammate server | MCP/HTTP | sidecar/env | **半**：runtime 仍桌面；kernel 仅 profiles |
| PTY / pane | AppState + engine | terminal | rdg pty | **分叉** |

## 深根

- 独立 `ridge-kernel` 进程：control + domain 只读 + 最小 MCP。
- 「退出桌面端」= hide UI；彻底退出 = shutdown kernel → 外壳自退。

## 下一切片

1. kernel 挂 Git status 只读  
2. teammate HTTP 迁入 kernel 或 register-teammate 到 kernel.json  
3. 编组 SSOT 后端化  

