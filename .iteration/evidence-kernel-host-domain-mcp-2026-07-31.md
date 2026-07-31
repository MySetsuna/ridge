# 证据 · KERNEL HOST/DOMAIN/MCP（2026-07-31）

## 命令与结果

```
cargo test -p ridge-kernel          → 3 passed (domain agents, mcp init/list/call)
cargo test -p ridge-mcp-bridge --lib → 7 passed
cargo test -p ridge --lib kernel_lifecycle → 5 passed

rdg kernel ensure  → kernel ready pid=… port=…（日志: listening control+domain+mcp）
rdg kernel status  → health=ok
rdg kernel ensure  → 二次 attach 同 pid（禁双开）
rdg kernel agents  → {"ok":true,"source":"ridge-kernel","profiles":[claude,codex,…]}
rdg kernel fs-list C:\code\wind → {"ok":true,"source":"ridge-kernel","page":{…}}
rdg kernel mcp-smoke → tools/list 含 ridge_kernel_list_agents / ridge_kernel_fs_list
rdg kernel stop    → kernel stopped；status → 内核未登记
```

脚本：`scripts/kernel-host-smoke.ps1`

## 对照验收

| REQ | 项 | 证据 |
| --- | --- | --- |
| HOST-01 | ⑤ detect-or-spawn / 双 ensure | ensure×2 同 pid |
| HOST-01 | ⑥ CLI stop | stop → 未登记 |
| HOST-01 | ③ rdg Y/N | 代码 `confirm_quit_kernel_with_desktop`；SPEC 已改 Y/N |
| HOST-01 | ④ 内核死后外壳自退 | 桌面 `spawn_kernel_death_watcher`；rdg `SAW_KERNEL` 联动（真机 GUI 未在本证据跑） |
| HOST-01 | ① 退桌面 hide 内核仍在 | tray 只 hide；内核独立进程 |
| DOMAIN-01 | FS 真路径经内核 | `rdg kernel fs-list` |
| DOMAIN-01 | Agent profiles 经内核 | `rdg kernel agents` |
| DOMAIN-01 | 内核退后 fail-closed | stop 后 agents 不可用 |
| MCP-01 | 无 Tauri tools/list | `mcp-smoke` |
| MCP-01 | bridge 发现 kernel.json | `ridge-mcp-bridge` discover 链 |
| MCP-01 | 文档拓扑 | `docs/mcp-integration.md` |

## 仍未声称全关的部分

- DOMAIN：Git/Remote/PTY/编组 SSOT 尚未迁入内核进程（本切片只读 FS+profiles）。
- MCP：内核面仅领域只读工具；完整 split/roster 仍走桌面 teammate sidecar（有桌面时优先 env/sidecar）。
- HOST：真 GUI 双启桌面 + 杀内核后桌面进程退出未截屏；托盘交互人工验。
