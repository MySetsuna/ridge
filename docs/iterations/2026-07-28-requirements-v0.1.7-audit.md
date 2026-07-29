# v0.1.7 需求—实现—发布审计

日期：2026-07-28  
审计基线：`v0.1.7` → `5f65140924366ae9c9bae706421a8bcd7948b822`  
当前 HEAD：`fbde55d58e1b82446730f5b166aa0dc09ab35286`

## 判定口径

| 结论 | 证据要求 |
| --- | --- |
| 已进入 v0.1.7 安装包 | 对应实现提交为 `v0.1.7` 祖先，且三处 manifest 版本均为 `0.1.7` |
| 已发布 | GitHub Release 非 draft/prerelease，目标为 `5f65140`，版本一致资产已上传 |
| 代码侧已验 | 对应迭代报告记录确定性测试及退出码；仅据报告，不上推为真实生产体验 |
| 真链已验 | 报告明确包含真实 LAN/public/cross-account/production 链路；缺项逐项保留 |
| 未进入 v0.1.7 | 实现提交位于 `v0.1.7..HEAD`，或仅有需求/合同而无实现 |

“安装包包含代码”“自动测试通过”“真实链路通过”“生产已部署”四者互不替代。

## Release 事实

- `v0.1.7` 为已发布正式 Release：`isDraft=false`、`isPrerelease=false`，发布时间 `2026-07-28T08:22:36Z`。
- tag 与 Release 均指向 `5f65140924366ae9c9bae706421a8bcd7948b822`。
- `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 在 tag 中均为 `0.1.7`。
- 资产共 11 项且版本相符：
  - Windows：setup EXE、MSI、`rdg` EXE；
  - Linux：DEB、AppImage、`rdg`；
  - macOS：aarch64/x64 DMG、两份 app tar、aarch64 `rdg`。
- `v0.1.8` 当前仍为 draft，`publishedAt=null`；其资产存在不等于已发布。

Release：https://github.com/MySetsuna/ridge/releases/tag/v0.1.7

## Active 需求逐条结论

| 需求 | v0.1.7 结论 | 直接证据 | 尚缺证据 / 纠偏 |
| --- | --- | --- | --- |
| `REQ-MOBILE-REMOTE-STATE-01` | **未进入 v0.1.7** | 实现 `9bbfb5e feat(remote): keep pane state continuous under weak networks` 不为 `v0.1.7` 祖先；仅在 `v0.1.7..HEAD` | iteration 63 虽报告真 LAN E2E，但用户实测已否定体验闭环；修订已作为 `REQ-REMOTE-SMOOTH-STATE-02` / `v0.2.5` 获批，禁止沿用“关闭”措辞 |
| `REQ-AGENT-HISTORY-01` | **未实现、未进入 v0.1.7** | 批准/合同归档 `0df0f04` 不为 `v0.1.7` 祖先；`CONTRACT-iteration-64.md` 状态为 APPROVED | 当前 UI 尚非“成员 / 编组 / 历史”三 Tab，结构化恢复与 adapter 证据尚无代码/测试 |
| `REQ-REMOTE-HOST-TREE-01` | **实现已随 v0.1.7 发布；真链未闭** | `0e71da6 feat(hosts): add managed remote resource forest` 为 tag 祖先 | iteration 62 明记仍缺真实 LAN/public browser/host fixture E2E |
| `REQ-WORKSPACE-SHARE-01` | **实现已随 v0.1.7 发布；真链未闭** | `186b9b6`、`f8b6141`、`9adfc4d`、`08eeff6` 均在 tag 内 | 缺跨账号邀请/接受/打开、读写/Git/Agent、pane 动态、撤销踢线与二跳拒绝真实 E2E；仅代码关闭 |
| `REQ-WORKSPACE-SAVED-01` | **修复已随 v0.1.7 发布** | `fe37599 fix(workspace): reopen and delete saved workspaces` 为 tag 祖先 | 尚未取得独立真实用户操作证据；不得扩大为全部 workspace 生命周期已验 |
| `REQ-REMOTE-03` | **多轮几何修复已随 v0.1.7 发布；公网生产未闭** | `96ce9fc`、`1d3a347`、`5d20854`、`281bf62`、`8e6ec6f` 等均在 tag 内 | 有 LAN/fixture 证据；iteration 62 与 PROJECT-STATE 均保留公网真实控制器/WebRTC E2E 缺口 |
| `REQ-REMOTE-01` | **实现已随 v0.1.7 发布** | `cbdd2d8 fix(remote): make rdg startup explicit and route LAN directly` 为 tag 祖先 | iteration 61 要求的显式启停与 LAN URL/boot 已落；真实多设备用户轨不由安装包证明 |
| `REQ-REMOTE-02` | **实现已随 v0.1.7 发布** | `cbdd2d8` 及 `remoteBootMode` 测试在 tag 内 | LAN 桌面浏览器启动路径有确定性测试；部署网络环境仍属运行证据 |
| `REQ-CLOUD-01` | **代码侧已完成，但不能仅凭 v0.1.7 宣称服务端上线** | iteration 61 记录 ridge-cloud quota 测试 `6/6` | 配额逻辑位于独立 ridge-cloud 仓库/部署线；wind v0.1.7 资产不证明服务端生产部署 |
| `REQ-MOBILE-01` | **UI 修复已随 v0.1.7 发布** | `50fd934 fix(mobile): portal workspace sheets and simplify agent actions` 为 tag 祖先 | 只证明 portal/icon/action 代码；手机真机触控仍须用户轨 |
| `REQ-AGENT-01` | **iteration 61 能力已随 v0.1.7 发布；本次状态同步缺陷未闭** | `6d5c381 feat(agents): unify roster replies and headless sessions` 为 tag 祖先 | Agent Tab 运行中而 pane header 未同步为新确认 bug；最近回复亦将由历史页取代 |
| `REQ-AGENT-02` | **实现已随 v0.1.7 发布** | `6d5c381` 与后续 `6864fb5 fix(agents): defer auto discovery to backend` 均为 tag 祖先 | 自动发现/唤醒代码存在；退出后 roster 与 pane header 一致性仍须本次修复 |

## 提交边界

### v0.1.7 内

- iteration 61：`cbdd2d8`（rdg/LAN boot）、`50fd934`（Mobile portal/icon）、`6d5c381`（Agent Center/无头 session）、`6864fb5`（后端自动发现）。
- iteration 62：`fe37599`、`96ce9fc`、`186b9b6`、`f8b6141`、`afaac0c`、`9adfc4d`、`0e71da6`、`08eeff6`。
- Remote 几何与统一产物：`1d3a347`、`67e0631`、`5d20854`、`281bf62`、`4b22be6`、`8e6ec6f`、`6bf7fc9`。

### v0.1.7 之后

- `9bbfb5e`：iteration 63 Mobile Remote continuity 实现。
- `0df0f04`：iteration 63 关闭文档与 Agent history 批准/合同。
- `a1d81d4`：Sonar 配置。
- `fbde55d`：准备 v0.1.8；对应 Release 仍为 draft。

## 最终判定

1. **确已上线 v0.1.7 安装包**：Remote 入口/LAN boot、Mobile portal/icon、Agent Center/无头发现、host 树、workspace share/saved、Remote 几何与统一产物代码。
2. **虽随包交付但不可称体验闭环**：host tree、workspace share、Remote 几何；真实 LAN/public/cross-account/production 矩阵仍有缺口。
3. **明确不在 v0.1.7**：iteration 63 Mobile continuity；其后虽进入 main/v0.1.8 draft，用户实测仍失败。
4. **明确尚未实现**：Agent 历史页/结构化恢复；既有 iteration 64 合同未丢失。
5. **不能由 v0.1.7 Release 证明上线**：ridge-cloud 服务端 quota 部署；需独立部署/版本证据。
