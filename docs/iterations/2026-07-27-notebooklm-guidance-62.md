# NotebookLM 指导 62：主机树、工作区分享与浏览器几何

来源：`PROJECT-STATE` + `REQUIREMENTS-SPEC`（2026-07-27 替换后双来源）。

## Maker 原建议

NotebookLM 要求同轮覆盖三项 Active，顺序为：

1. wind 几何 SSOT 与鼠标命中修复。
2. ridge-cloud grant、邀请与 scoped token。
3. wind host→workspace→pane DTO/树。
4. wind scoped provider，把共享区接入 Terminal/Explorer/Git/Agent。

其安全建议：完整 host 同账号门禁不动；grant 绑定
`owner/device/workspace/grantee/role/expiry`；首版可 operator-only；撤销/过期
fail-closed；共享资源不可二次转发。其测试建议：分数 DPR 纯函数、异账号越界合同、
撤销踢线、R62-SAVED 回归。

## Checker

| 原建议 | 代码事实 | 裁决 |
| --- | --- | --- |
| 新房间 `share:{grant_id}` | host 当前只加入 owner/device room；独立 share room 无 host，若令 host 为每个 grant 重入则放大状态与连接 | **reframe**：scoped controller 仍路由 owner/device room；RoomRegistry 保存 controller grant 元数据并可按 grant 踢线 |
| `cellWidth * dpr` 换算 pointer | `clientX/getBoundingClientRect/cellW` 均为 CSS px；乘 DPR 会在 125%/150% 缩放再次放大 | **驳回**：DPR 只用于 renderer device viewport；pointer 用同一 CSS geometry 的 grid origin/cell |
| 新增 16ms throttle + final sync | 全局 host ResizeObserver 已逐帧合并；manager 已有 500ms trailing fit 与 pointerup flush | **驳回**：复用现有有界机制；只统一几何快照与保证最终 claim |
| `remoteAllowlist.ts` | 实际 SSOT 为 Rust capability + TS mirror/合同，并无该文件 | **reframe**：沿现有 `REMOTE_ALLOWLIST`/capability parity 增 scope policy，不造协议副本 |
| relay 单发 `FORCE_DISCONNECT` | 已有 RoomRegistry controller sender/cid 生命周期 | **reframe**：DB revoke 后以 grant→room/cid 索引踢线；host 收 peer-leave 清 scoped bridge |
| viewer/operator 同发 | 文件、Git、Agent 写面多，若任一只靠 UI 即伪只读 | **收窄**：v1 operator-only；API/UI 稳定拒绝 viewer，保留模型枚举待后续闭环 |
| 新 `get_host_topology` | `RemoteLink` 已有 workspace/pane list 与 CRUD | **驳回**：store adapter 聚合既有原语；不扩平行 RPC |

## 最终采纳

- 三项 Active 均进 iteration 62；R62-SAVED 同轮回归。
- 几何先行：共享纯函数产出 content rect、grid、CSS/device viewport、pointer origin。
- full-host 与 workspace-share 共 UI forest、Remote RPC，不共授权。
- Cloud 数据库为 grant 真相；短 token 每次 WS upgrade 重查；完整 host 同账号判断不放宽。
- host 必须逐 invoke、pane subscribe、event 验 scope；relay 不读 E2EE 业务明文。
- v1 operator-only；共享区仍不可创建/关闭 workspace、转分享、host/Remote 二跳。
- 三个桌面入口调用同一分享对话框；共享 projection 不写本地 workspace graph。

## 第二轮：桌面 scoped-provider 接法

NotebookLM 建议以独立、内存态投影承载 `grantId + workspaceId`，复用 Remote
终端、文件、Git、搜索与 Agent 资源；关闭时整体销毁，不写本机工作区图。

| 原建议 | 裁决 | 校正 |
| --- | --- | --- |
| 建独立 projection store | **采纳并收窄** | 单活动投影；只存授权 workspace、pane 与独立连接/provider；不持久化 |
| 客户端注入授权 metadata / `RpcClient.scoped` | **驳回** | metadata 不构成授权；host 只信 ridge-cloud `workspace_share` JWT 与 host 侧 `planWorkspaceInvoke` |
| 新建 `CloudHostBridge.rs` | **驳回（符号不存在）** | 实际桥为 TS `cloudHostBridge.ts`；桌面侧复用 `TauriBridge + RpcClient` |
| 调用 `remote_read_dir` 等新 RPC | **驳回（方法不存在）** | 复用 `get_file_tree/read_file/get_scm_status/text_search/get_teammate_topology` |
| 困难时退化为纯终端 | **驳回** | 违反已批准需求；失败须 fail-closed，不得隐去 Explorer/Git/Agent |

锁定实现：

- `getWorkspaceShareToken` 取得不可委派 token；客户端复核 grant/workspace/device 与 `delegable=false`。
- 每个投影创建独立 `TauriBridge/RpcClient/CloudRemoteConnection`；不安装 global transport，不发 `use-global-workspace`。
- `TauriDataProvider` 接受显式 invoker；桌面 Files/Git/Search/Agent 与嵌入终端共用该 provider。
- `CloudRemoteConnection` 保留既有默认 Tauri API 路径；仅共享场景显式注入独立桥。
- workspace 管理关闭、pane operator 操作保留；host/remote desktop API 仍由 scope deny-list 全拒。

自动证据：Vitest 107 文件、1255 通过、1 跳过；svelte-check 0 error；
Remote 与桌面 production build exit 0。尚欠跨账号真实链路 E2E。
