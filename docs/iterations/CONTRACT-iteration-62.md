# CONTRACT — Iteration 62 / Host Tree、Workspace Share、Remote Geometry

需求：`REQ-REMOTE-HOST-TREE-01`、`REQ-WORKSPACE-SHARE-01`、
`REQ-REMOTE-03`；回归 `REQ-WORKSPACE-SAVED-01`。

设计：

- `docs/specs/2026-07-27-host-tree-workspace-sharing-design.md`
- `docs/specs/2026-07-27-remote-browser-geometry-design.md`

基线例外：wind `8185cd8` 相对 `origin/main` ahead 7 / behind 0，用户已明确要求
继续推进；ridge-cloud `a5e2be6` 与 `origin/main` 同步。两仓均在 `main`。

## 切片 1：浏览器几何 SSOT

- 抽最小 `PaneGeometry` 纯函数；manager 单次读 DOM 后统一计算 grid、viewport、
  claim pixel 与 pointer origin。
- 修 shared letterbox 点击偏移；CSS/device px 不混算。
- 初挂、split/sidebar/window、DPR、reconnect 均走既有有界 resize 生命周期。
- 验收：DPR/分数/padding/居中/裁剪纯函数测；manager 合同测；LAN/public browser
  fixture 的 grid/claim/pointer 协议断言。

## 切片 2：Cloud workspace grant

- 追加 migration 与 repo：grant 绑定 owner user、device、workspace、grantee、
  operator role、status/expiry；v1 明拒 viewer。
- owner create/list/revoke；grantee inbox/accept/decline；active grant 换短期 scoped
  token。用户名/email 定向，不发 bearer 邀请链接。
- scoped controller 经 owner/device room 接入；WS upgrade 每次查 DB 真相；完整 host
  `scope=user + same account` 不放宽。
- RoomRegistry 记录 grant→cid，撤销/过期踢既有连接；token 重放不可恢复。
- 更新 `ridge-cloud-protocol.md` canonical 全文及协议守卫。
- 验收：API/repo/WS/room 测覆盖异账号正常分享、sibling/role/host token 越权、
  revoke/expiry/replay、viewer 拒绝。

## 切片 3：Host 二次 scope 门禁

- E2EE session 建立后，host 取得可验 scoped assertion；每 controller 建
  `WorkspaceShare` policy。
- 所有 invoke、pane subscribe/output/event 先过单一 policy；workspace/pane/path/
  repo/agent 归属由 host 反查，不信任 controller 自报。
- operator 可目标 workspace 内既有写；禁止 create/close workspace、share、host/
  Remote/export；危险写仍走 HITL。
- shared projection 永不进入本机 workspace graph、host inventory 或 Remote export。
- 验收：跨 workspace pane/file/Git/Agent 与二跳负合同；撤销 peer-leave 清订阅。

## 切片 4：三层 Hosts forest

- `hosts.ts` 以 adapter 聚合现有 `RemoteLink.listWorkspaces/listWorkspacePanes`；
  不增 `get_host_topology` 平行 RPC。
- 渲染 host→workspace→pane；菜单按 capability/scope 收敛。
- pane 删除仅删目标；成功后按该控制端同源 host 已接入 pane Set 计数，归零断连
  一次，有第二 pane 保持；失败不减。
- LAN 不施加 cloud 账户限制；公网 full-host 仍同账号。
- 验收：双 host fixture 隔离、展开单飞刷新、动作目标、删 pane 三分支。

## 切片 5：共享工作区桌面接入

- Explorer 标题、WorkspaceTree 行、Hosts owner workspace 三入口调用同一
  `WorkspaceShareDialog`。
- grantee Hosts 显示受限共享根；打开后 Terminal/Explorer/Git/Agent 共用
  `grantId+workspaceId` scoped provider。
- pane cwd 与本机 cwd 同层同形；文件打开与 operator 写皆落 origin root。
- shared projection 无再次分享、workspace 管理、Remote/Hosts 转发入口。
- 验收：三入口同 command/dialog；cwd/open-file；Git/Agent scope；operator 菜单；
  inventory/export 负断言。

## 回归与总闸

- R62-SAVED：paneTree 关闭清 runtime、受限 `.ridge` 删除、`rg-scroll`。
- wind：聚焦 Vitest/Rust → `pnpm check` → 相关 cargo → LAN/public Remote build/E2E。
- ridge-cloud：`cargo fmt --check`、`cargo test`。
- `requirements_gate.py assert-executable` exit 0。
- 每 concern 独立提交；最终两仓工作树干净。

## 停止/回滚

- 任一 share 请求可见 sibling workspace/pane/event：停止发布，回滚 share 切片。
- host 未逐请求验 scope、撤销不能踢既有 cid、shared 可二跳：不得声称完成。
- viewer 只读链未全闭：保持 v1 operator-only，API/UI 不出现 viewer。
- 几何若需 transport 各写算法或新增无界观察器：停止，回收至 manager SSOT。
- Cloud migration 只追加；回滚停用路由/签发，不删生产 grant 数据。

## 减法证明

- 不建 share 专属业务协议/独立房间；复用 owner room、E2EE、Remote RPC。
- 不建 topology RPC；聚合既有 workspace/pane 原语。
- 不把 shared workspace 复制成本地 workspace。
- 不做 viewer、匿名链接、LAN share 优化、pane 缩略图。

