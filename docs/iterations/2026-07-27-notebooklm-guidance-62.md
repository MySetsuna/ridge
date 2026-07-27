# NotebookLM 指导 62：多主机树与工作区分享

## Maker 原建议

NotebookLM 建议一轮覆盖真实 Socket、三层 DTO、store、多级 UI、headless 归因与远程 CRUD；主张 LAN/public 复用既有 Remote 原语。

## Checker

| 原建议 | 代码事实校验 | 裁决 |
| --- | --- | --- |
| 新建 `remote/lan_client.rs` + `tokio-tungstenite` | 本仓已有 `RemoteConnection`/Cloud provider；直接再造协议会成副本 | reframed：先以 adapter 复用 `RemoteLink`，transport 只注册现有 provider |
| LAN/public 统一用 `ControllerCloudProvider` | LAN 不应依赖云信令，现有 `RemoteConnection` 已是 LAN 腿 | reframed：统一接口与 policy，不统一 transport |
| 新建 `get_host_topology` | `RemoteLink` 已有 listWorkspaces/listWorkspacePanes/CRUD | reframed：Hosts store adapter 聚合既有原语，避免平行 RPC |
| headless 加入远端三层树 | native headless 是本机 provider，creator DTO/Agent Center/summon 已接 | 采纳“补证据”，不混入 workspace-share 授权 |
| TLS 失败降级手动注入 socket | 手动注入不是生产能力，且会制造假完成 | non-goal；替代：显示 unsupported/error，不伪称已接入 |
| >200ms 即停止轮询 | 无现成可判定基线，阈值臆造 | reframed：展开按需 + 每 host 单飞刷新 + 队列有界确定性测 |

## 新增用户约束后的锁定方向

- full-host：同账号、全 host scope。
- workspace-share：可跨账号、单 workspace capability grant。
- 两者统一 UI forest 与 RPC policy，不统一授权范围。
- 产品行为仍在 `REQUIREMENTS-SPEC` Pending；获批前不生成执行合同、不改业务码。
