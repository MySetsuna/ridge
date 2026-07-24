# 开放愿景清单（Ridge）

更新：2026-07-24（iteration 16）  
规则：Notes 清空 ≡ 愿景全实现；`[已实现]` 视同清理。

| ID | 主题 | 状态 | 验收证据 |
| --- | --- | --- | --- |
| V-H1 | TCP 可达探测 | **implemented** | hosts probe 测 |
| V-H1-LIVE | 出站 live 输入路由 + attach 门控 | **implemented**（最小闭环） | `live_sink_routes_bytes`；`attach_host_session`；write 经 remote_ref。完整 WS 字节回灌仍待实机 host |
| V-G1-OS | OS 冻结 pid | **implemented** | os_freeze 测 |
| V-G1-JOB | PTY spawn 预建 Job + assign | **implemented** | job_object 测；spawn 路径挂 job |
| V-G1-RB | git checkpoint/rollback | **implemented** | rollback 测 |
| V-M1-S3 | memory goal/constraints/tasks | **implemented** | memory 测 |
| V-B6A | REMOTE_UI_MISSING | **implemented** | ridge-remote 测 |
| V-B3 | 图片版本刷新 | **implemented** | vitest |
| V-DISC | CLI 探测 | **implemented** | discover 测 |
| V-MOB-CP | 移动复制 | **implemented** | mobileCopy 测 |
| V-TUI-CLK | 鼠标报告 | **implemented** | 既有 |
| V-B6B | resize 尺寸 | **implemented** | resize 测 |
| V-PASTE | 多行粘贴序 | **implemented** | paste 测 |

**open 计数：0**

## 明确不纳入愿景（运维 / Level 2）

| 项 | 理由 |
| --- | --- |
| 真机 smoke | 物理设备证据 |
| 生产 Dokku / Remote 云发布 | 需 `RIDGE_ARTIFACT_TOKEN`；本机无 token 时换机发 |
| 分支 merge 进 main | Level 2 人工审查 |

## 下一里程（非 open 阻塞）

- V-H1-LIVE 完整：LAN/WS 出站客户端复用 `ridge-cli` lan_session 语义，foreign pane 真 PTY 视图 + 回灌。
