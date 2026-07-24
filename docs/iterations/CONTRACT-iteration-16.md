# CONTRACT — Iteration 16：有价值「非功能」愿景纳入 + 发布

日期：2026-07-24

## 评估结论（拍板）

| 项 | 建议 | 理由 |
| --- | --- | --- |
| **完整出站 PTY 流**（V-H1-LIVE） | **纳入并落地最小可测闭环** | Hosts 设计核心；仅 TCP 探测不等于「连上能用」。价值高。本轮交付：Connected 主机可 `attach_host_session` 建 foreign 视图 + 输入路由钩 + mock 字节回灌测；真实 LAN WS 握手可测则接，否则显式 next。 |
| **Job Object 预建**（V-G1-JOB） | **纳入并落地** | Windows 真冻结目标态；预建 job 消进程树竞态。价值中高、改 spawn 单点可控。 |
| 真机 smoke | **不纳入愿景** | 物理设备证据，checklist 保留，非代码缺口。 |
| 生产 status/Dokku | **不纳入愿景** | 需 `RIDGE_ARTIFACT_TOKEN`/生产权限；本机无 token → Remote 云发布延后换机。 |
| 分支 merge | **不纳入愿景** | Level 2 人工审查合并，不自动 merge。 |

## 目标

1. V-G1-JOB：Windows PTY spawn 后 AssignProcessToJobObject；`os_freeze` 优先经 job；无 job 回落 pid。Unix 仍 SIGSTOP。单测：job create/assign API 不 panic；freeze 路径可测。
2. V-H1-LIVE：HostRegistry 可挂 live sink；`attach_host_session` 仅 Connected；建 `remote_ref` foreign pane；`write` 经 host 路由；mock 测绿。
3. 开放清单更新；PROJECT-STATE/LOG；note 同步。
4. 版本 **0.0.18** Release：改 version、构建安装包；`gh release` 若可行。Remote 云发布：有 token 则 `publish:remote-cloud`，否则文档写「换机发」。

## 不做

E2EE/协议 SSOT 扩张、auto-merge、Job freeze 未文档 API 强依赖失败即崩（须 fail-open）。
