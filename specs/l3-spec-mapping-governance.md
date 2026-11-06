---
id: L3-SPEC-MAPPING-GOVERNANCE
level: L3
title: 规范映射处置及可校验历史证据
status: LOCKED
lifecycle: ACTIVE
parent: L2-UNIFIED-CLIENT-RUNTIME
code_targets:
  - docs/archive/CHG-090/source/src-tauri/src/commands/terminal.rs
  - docs/archive/CHG-090/source/src-tauri/src/remote_host_impl.rs
  - docs/archive/CHG-090/source/packages/ridge-cli/src/daemon_ctl.rs
  - docs/archive/CHG-090/source/packages/ridge-cli/src/session.rs
  - docs/archive/CHG-090/source/packages/ridge-cli/src/tui/lan_host_impl.rs
  - docs/archive/CHG-090/source/packages/ridge-cli/src/tui/lan_host.rs
  - docs/archive/CHG-090/source/packages/ridge-cli/src/tui/scrollback.rs
  - docs/archive/CHG-090/source/packages/ridge-cli/src/tui/workspace.rs
  - docs/archive/CHG-090/source/src/lib/terminal/ptyRuntimeSnapshot.ts
  - docs/archive/CHG-090/source/src-tauri/src/remote_bridge.rs
  - docs/archive/CHG-090/source/src-tauri/src/commands/cloud_pane.rs
  - docs/archive/CHG-090/source/src-tauri/src/commands/fs_watch.rs
  - docs/archive/CHG-090/source/src-tauri/src/commands/pane.rs
  - docs/archive/CHG-090/source/src-tauri/src/commands/process.rs
  - docs/archive/CHG-090/source/src-tauri/src/commands/remote.rs
  - docs/archive/CHG-090/source/src-tauri/src/commands/settings.rs
  - docs/archive/CHG-090/source/src-tauri/src/commands/teammate.rs
  - docs/archive/CHG-090/source/src-tauri/src/commands/workspace.rs
  - docs/archive/CHG-090/source/src-tauri/src/db/mod.rs
  - docs/archive/CHG-090/source/src-tauri/src/db/projects.rs
  - docs/archive/CHG-090/source/src-tauri/src/hosts/mod.rs
  - docs/archive/CHG-090/source/src-tauri/src/hosts/outbound.rs
  - docs/archive/CHG-090/source/src-tauri/src/lsp/mod.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/agent_catalog.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/autodiscover.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/context_files.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/hitl.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/mcp.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/memory.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/mod.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/orch_health.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/rollback.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/server.rs
  - docs/archive/CHG-090/source/src-tauri/src/teammate/suspend.rs
  - docs/archive/CHG-090/source/packages/ridge-cli/src/session_control_tests.rs
  - docs/archive/CHG-090/source/static/mobile/assets/index-BPfF1vzx.css
  - docs/archive/CHG-090/source/static/mobile/assets/index-Cm2sAwTC.js
  - docs/archive/CHG-090/source/src/lib/terminal/ptyRuntimeSnapshot.test.ts
  - docs/archive/CHG-090/.gitattributes
test_targets:
  - scripts/check-spec-mappings.mjs
acceptance_checks:
  - id: spec-mapping-audit
    command:
      - node
      - scripts/check-spec-mappings.mjs
    description: 核对 98 份 Git 源证据、所有节点处置、现役测试路径及审计零失效目标
---

# 规范映射治理与历史证据

## 范围

CHG-090 治理初始审计 180 条失效引用，涉及 107 个节点、91 个路径。四个现役需求节点重绑 Kernel 和对应测试，旧 M5.3 三子节点及父节点依 CHG-070 归档。迁移观察节点保留现存目标，完全失效者退役；两个既有归档资产节点修正历史证据路径，空的旧资产父目录一并归档。三个活测试从已退役父目录迁至真实前端模块。

## 验证扩展

verify 另发现审计未覆盖的失效测试引用。全图核对共 190 条、74 个观察节点、7 个测试路径。旧测试原文保留，失效测试观察节点归档，活节点清除过时测试引用。九组接口快照逐项核对既有变更与真实导出，由确定性 interface 命令更新，本轮不更改产品行为。

## 证据及验收

每条源路径对应最后存在的 Git commit 与 blob。仅在现有归档 blob 完全一致时复用，否则补存原 blob，历史 code_targets 不表达现役实现。处置节点保留原需求及原始映射。确定性检查调用实际 stc graph/audit，核对全部历史 blob、节点生命周期、现役映射无归档证据路径和零失效目标。测试名称不代表已执行的跨平台/公网验收。

其余 MISSING_TEST、EMPTY_AREA、SHARED_TEST_TARGET、UNDECLARED_DEPENDENCY、UNCOVERED_FILE 告警保留分类计数，属于独立追踪范围。不得用通配映射补齐率或删除要求掩盖问题。执行 npm test、lint、typecheck、SpecTree verify/complete。

## 处置证据

```json
{
  "sources": [
    {
      "old": "src-tauri/src/commands/terminal.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "15029d6855a7c6d1aba44e8a33a5d3d82961a7bd",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/commands/terminal.rs"
    },
    {
      "old": "src-tauri/src/teammate/job_object.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "cda64112ca16d80bddcf567545390e931db606b3",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/job_object.rs"
    },
    {
      "old": "src-tauri/src/remote_host_impl.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "4494aeb7dec320fa924d5ddcb45ff6e29705efb1",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/remote_host_impl.rs"
    },
    {
      "old": "packages/ridge-cli/src/daemon_ctl.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "32e24ff695afe625da0c51c1bc6d6f7b3fba5f3f",
      "archive": "docs/archive/CHG-090/source/packages/ridge-cli/src/daemon_ctl.rs"
    },
    {
      "old": "packages/ridge-cli/src/session.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "c59ff2a3b42c42d4b999f845475ed8b933776587",
      "archive": "docs/archive/CHG-090/source/packages/ridge-cli/src/session.rs"
    },
    {
      "old": "packages/ridge-cli/src/tui/lan_host_impl.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "4495c07846c390e1e895662e8bb1bee6f562877b",
      "archive": "docs/archive/CHG-090/source/packages/ridge-cli/src/tui/lan_host_impl.rs"
    },
    {
      "old": "packages/ridge-cli/src/tui/lan_host.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "21bf57c48a8f624f69b495f8ff16ccdcbe881946",
      "archive": "docs/archive/CHG-090/source/packages/ridge-cli/src/tui/lan_host.rs"
    },
    {
      "old": "packages/ridge-cli/src/tui/scrollback.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "ca1aa593e112fd612ba5300ccdf54c5a0766b205",
      "archive": "docs/archive/CHG-090/source/packages/ridge-cli/src/tui/scrollback.rs"
    },
    {
      "old": "packages/ridge-cli/src/tui/workspace.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "155dd48292448423a553bb8f9da114c6cc118e51",
      "archive": "docs/archive/CHG-090/source/packages/ridge-cli/src/tui/workspace.rs"
    },
    {
      "old": "src/lib/remote/cloud/cloudHostTopologyLink.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "fe7afc793d3a41e2200c160861ed33f9e0306a0a",
      "archive": "docs/archive/CHG-070/frontend-cloud-topology/cloudHostTopologyLink.ts"
    },
    {
      "old": "src/lib/stores/hostReconnect.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "5bfb84c135e5e9671656ee061db115f06eb7c891",
      "archive": "docs/archive/CHG-070/hostReconnect.ts"
    },
    {
      "old": "src/lib/teammate/layoutChange.golden.json",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "c11ef79581b0e8fb05b49e52dbe05fed3b40395e",
      "archive": "docs/archive/CHG-070/frontend-runtime-events/layoutChange.golden.json"
    },
    {
      "old": "src/lib/teammate/layoutEvent.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "872c965fe4b34a531eb81b364b36c51608fbdc15",
      "archive": "docs/archive/CHG-070/frontend-runtime-events/layoutEvent.ts"
    },
    {
      "old": "src/lib/terminal/ptyRuntimeSnapshot.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "10cc67ef6863f76e130c5d8399ee28b7c95eedf1",
      "archive": "docs/archive/CHG-090/source/src/lib/terminal/ptyRuntimeSnapshot.ts"
    },
    {
      "old": "src-tauri/src/remote_bridge.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "e056410a768ccc8cd0313171fdf1da3697a4c60e",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/remote_bridge.rs"
    },
    {
      "old": "src-tauri/src/types.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "76eae6a7f0de58929ae5a488485acc85c8c6c2b8",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/types.rs"
    },
    {
      "old": "src-tauri/src/commands/cloud_http.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "30ce95dbef08dcc0ad1c266718866e884d7b24a9",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/commands/cloud_http.rs"
    },
    {
      "old": "src-tauri/src/commands/cloud_pane.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "b42df5590aa4eb8a55ccf6328b05afd556a96ca5",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/commands/cloud_pane.rs"
    },
    {
      "old": "src-tauri/src/commands/fs_watch.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "05bcf133ccd1a69d2b53bb6b929f836469909e9e",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/commands/fs_watch.rs"
    },
    {
      "old": "src-tauri/src/commands/git.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "3a31c2889f45436f8ab251a3f21f4f00cb588c16",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/commands/git.rs"
    },
    {
      "old": "src-tauri/src/commands/pane.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "314343138789235b4d229360e6cc922d96b9c853",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/commands/pane.rs"
    },
    {
      "old": "src-tauri/src/commands/process.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "88179974352aa243b3d8c3395edf80d9849c8b48",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/commands/process.rs"
    },
    {
      "old": "src-tauri/src/commands/remote.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "523971b401e85e70bc76866e31740e21e6d07b3e",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/commands/remote.rs"
    },
    {
      "old": "src-tauri/src/commands/settings.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "5ecd5352926bdbf1da3e1336519dae6e3024a442",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/commands/settings.rs"
    },
    {
      "old": "src-tauri/src/commands/teammate.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "9d11e0a2830d58ac86dc2dc98c12b6eaa0471b98",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/commands/teammate.rs"
    },
    {
      "old": "src-tauri/src/commands/terminal_font.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "1388ab7cc3282f2ecfac2e4dbafd5b02b4e5aa2e",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/commands/terminal_font.rs"
    },
    {
      "old": "src-tauri/src/commands/watch.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "0d505201fba8399eb95a2d68958dd637e5e5af1f",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/commands/watch.rs"
    },
    {
      "old": "src-tauri/src/commands/workspace.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "8a5204390d3311867474b6654f8ecc54317fe88e",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/commands/workspace.rs"
    },
    {
      "old": "src-tauri/src/db/mod.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "e539c47b0a2ca72f05f839d880def8c2f8d3ba0e",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/db/mod.rs"
    },
    {
      "old": "src-tauri/src/db/projects.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "cd248f3c8168275643700bc881f1ff880878be0e",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/db/projects.rs"
    },
    {
      "old": "src-tauri/src/engine/kernel_pty.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "2443eb631826581fced757b0d2935a32dcbd0049",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/engine/kernel_pty.rs"
    },
    {
      "old": "src-tauri/src/engine/mod.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "0ec125dadf50b0403fd16afd5a2bc8e814e871a8",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/engine/mod.rs"
    },
    {
      "old": "src-tauri/src/engine/pane_tree.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "34749886adc878c8d37bff7d2ecca393bc7025e9",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/engine/pane_tree.rs"
    },
    {
      "old": "src-tauri/src/engine/parser.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "6017aa2b200e47df2d6b6e1272be4a9b7d42b9fe",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/engine/parser.rs"
    },
    {
      "old": "src-tauri/src/engine/pty.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "b17874492947c3ba83ce50bd41e493c499ec5337",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/engine/pty.rs"
    },
    {
      "old": "src-tauri/src/fs/mod.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "68167ac6f934a6b2a6ea99a245ecfe10218fb414",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/fs/mod.rs"
    },
    {
      "old": "src-tauri/src/fs/search.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "a48b754c9f589b1f1a5e2ccc2c052e481ddeffa3",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/fs/search.rs"
    },
    {
      "old": "src-tauri/src/fs/tree.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "ebd1821d5d0948c3dc6b79e854497bcc53306fa7",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/fs/tree.rs"
    },
    {
      "old": "src-tauri/src/hosts/desktop_surface.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "699331ebf90b032a1a7216f774f7e6288bd2b403",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/hosts/desktop_surface.rs"
    },
    {
      "old": "src-tauri/src/hosts/foreign_history.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "3280024915321ad7365a3b255120ac7c76a619bb",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/hosts/foreign_history.rs"
    },
    {
      "old": "src-tauri/src/hosts/history_commands.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "cd7f02102c8ca711a272b9f41471f8b1463c3f5a",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/hosts/history_commands.rs"
    },
    {
      "old": "src-tauri/src/hosts/lan_transport.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "d0843215fcdbd1a01b1dd6a3d3c8eb9b4b1845e3",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/hosts/lan_transport.rs"
    },
    {
      "old": "src-tauri/src/hosts/live_backpressure.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "348a2655bc867c2c5d956a0b79778ab9a6658b85",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/hosts/live_backpressure.rs"
    },
    {
      "old": "src-tauri/src/hosts/mod.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "d6d1efabf77dbc2230bf96f8da10681b9315bb5b",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/hosts/mod.rs"
    },
    {
      "old": "src-tauri/src/hosts/outbound.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "5aed94526ad79ee3f1f41a5971c0b0827f5e5a0a",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/hosts/outbound.rs"
    },
    {
      "old": "src-tauri/src/hosts/reconnect_supervisor.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "300ab5e85063033f08ca5fb53e166fd9b4c1b7f0",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/hosts/reconnect_supervisor.rs"
    },
    {
      "old": "src-tauri/src/lsp/mod.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "317d06e55475b1e95771bc3094cdc6449c0d63b4",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/lsp/mod.rs"
    },
    {
      "old": "src-tauri/src/teammate/agent_catalog.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "78e60086f6e83ef37503315b76995e51d5ac24fc",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/agent_catalog.rs"
    },
    {
      "old": "src-tauri/src/teammate/autodiscover.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "ef9e997cb967d87e9ccd5db103cd116dec869c71",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/autodiscover.rs"
    },
    {
      "old": "src-tauri/src/teammate/circuit.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "91f5c7e4b43148582e573f54103313493874bbc1",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/circuit.rs"
    },
    {
      "old": "src-tauri/src/teammate/context_files.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "6e0fdfc8c558213df196063dba7641db372ee98b",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/context_files.rs"
    },
    {
      "old": "src-tauri/src/teammate/discover.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "529ccc31c0712b560ecf2e5794a9bd576de21b9c",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/discover.rs"
    },
    {
      "old": "src-tauri/src/teammate/endpoint.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "ba8addd8cde2fd9ee324a8210bb0ccb8db4d8295",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/endpoint.rs"
    },
    {
      "old": "src-tauri/src/teammate/hitl_audit.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "dd1086ac91f72a724fada1160aa72dc02e7d1aff",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/hitl_audit.rs"
    },
    {
      "old": "src-tauri/src/teammate/hitl.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "003960dc27977fa373628604084e46b884770e6e",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/hitl.rs"
    },
    {
      "old": "src-tauri/src/teammate/layout_event.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "3409ab811452a30bb892270c4c99f68ecc9d6ccc",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/layout_event.rs"
    },
    {
      "old": "src-tauri/src/teammate/locks.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "02051cc2291e4c401129e61eb722fdc483ce678b",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/locks.rs"
    },
    {
      "old": "src-tauri/src/teammate/mcp.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "b7e6f7585bff4237ae61b91b70375d4fa2b0f003",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/mcp.rs"
    },
    {
      "old": "src-tauri/src/teammate/memory.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "3e36cf4a11d5c2b799d26399f7952e3b0e190059",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/memory.rs"
    },
    {
      "old": "src-tauri/src/teammate/mod.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "a5e9556d265ebec74c108c859626822ac678cae4",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/mod.rs"
    },
    {
      "old": "src-tauri/src/teammate/native.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "347d3055204fba3270ac23c22ce6ccf72a0ec4f4",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/native.rs"
    },
    {
      "old": "src-tauri/src/teammate/orch_health.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "a6c96d2475a67852af9f5880c85a808fecb024f9",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/orch_health.rs"
    },
    {
      "old": "src-tauri/src/teammate/os_freeze.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "a3cc51e0f7e62788bd11af9309c58a1ffc3e6eff",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/os_freeze.rs"
    },
    {
      "old": "src-tauri/src/teammate/profiles.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "0d1d5001768a71db070c32ac268c92f0d312adea",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/teammate/profiles.rs"
    },
    {
      "old": "src-tauri/src/teammate/rollback.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "8854e53e3db0e2933859f4d52ec571ae9b456a7e",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/rollback.rs"
    },
    {
      "old": "src-tauri/src/teammate/server.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "b4aa90274ee41898af01bead435b5b9abc30c8c2",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/server.rs"
    },
    {
      "old": "src-tauri/src/teammate/suspend.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "b03f2dcfaa23d20e6da07abab9094b2a9ae26b7b",
      "archive": "docs/archive/CHG-090/source/src-tauri/src/teammate/suspend.rs"
    },
    {
      "old": "src-tauri/src/utils/error.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "9708f489e2353011292fbc05bd194fbdd60bab26",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/utils/error.rs"
    },
    {
      "old": "src-tauri/src/utils/pane_id.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "8d3be1b60dc3cb83fb199315b1eeac2aa1f5c872",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/utils/pane_id.rs"
    },
    {
      "old": "src-tauri/src/utils/pty_log.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "88ddd1580809ac76ee88a0bfe264fa951f749aed",
      "archive": "docs/archive/CHG-070/desktop-retired-tree/utils/pty_log.rs"
    },
    {
      "old": "packages/ridge-acp/tests/m53_acceptance.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "de84b876a7b035e9ccc2ce8124fd28ce452fd44a",
      "archive": "docs/archive/CHG-070/retired-communion/packages/ridge-acp/tests/m53_acceptance.rs"
    },
    {
      "old": "src/lib/agent-chat/productAcceptance.test.ts",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "ad9c2fbd83978c5c1e095c9a66601fe769889b44",
      "archive": "docs/archive/CHG-070/retired-communion/src/lib/agent-chat/productAcceptance.test.ts"
    },
    {
      "old": "src/lib/agent-chat/AgentChatPanel.svelte",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "cc38995c604e3aa877318447adaeb6397401356c",
      "archive": "docs/archive/CHG-070/retired-communion/src/lib/agent-chat/AgentChatPanel.svelte"
    },
    {
      "old": "src/lib/agent-chat/DefaultWiredAgentChat.svelte",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "439b4f1d30298213b873a26436e7ab4115914eab",
      "archive": "docs/archive/CHG-070/retired-communion/src/lib/agent-chat/DefaultWiredAgentChat.svelte"
    },
    {
      "old": "src/lib/agent-chat/AgentSessionPicker.svelte",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "212a31aab9577a30833a1aca26b78a8db21d633e",
      "archive": "docs/archive/CHG-070/retired-communion/src/lib/agent-chat/AgentSessionPicker.svelte"
    },
    {
      "old": "src/lib/agent-chat/agentEvents.ts",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "8e9c7feafc805d5407f6a29dff0c492b43b2ea57",
      "archive": "docs/archive/CHG-070/retired-communion/src/lib/agent-chat/agentEvents.ts"
    },
    {
      "old": "src-tauri/src/bridge/acp_bridge.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "9e282dff344f162fa4d31626770f16cb8fdc56f1",
      "archive": "docs/archive/CHG-070/retired-communion/src-tauri/src/bridge/acp_bridge.rs"
    },
    {
      "old": "src/lib/agent-chat/providerRestore.ts",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "858b04e4a91fb0d9a0f3dd39b87d7386d46539e9",
      "archive": "docs/archive/CHG-070/retired-communion/src/lib/agent-chat/providerRestore.ts"
    },
    {
      "old": "packages/ridge-acp/src/worker.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "89cb6da2c5ba14137d94e88e310ad8987acf7751",
      "archive": "docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/worker.rs"
    },
    {
      "old": "packages/ridge-acp/src/workflow.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "3685bcdcafceaa4461f159bab127114e1d4d028d",
      "archive": "docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/workflow.rs"
    },
    {
      "old": "packages/ridge-acp/src/test_harness.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "8278b0a706ca90215906ec507114122d98c0ad00",
      "archive": "docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/test_harness.rs"
    },
    {
      "old": "packages/ridge-acp/src/communion.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "6e1235bc655010f0474ed7f8ca1b2e55cfba56b4",
      "archive": "docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/communion.rs"
    },
    {
      "old": "packages/ridge-cli/src/session_control_tests.rs",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "eabcd2f1ba539adae729df1b2f31bef098e1523e",
      "archive": "docs/archive/CHG-090/source/packages/ridge-cli/src/session_control_tests.rs"
    },
    {
      "old": "packages/ridge-acp/src/provider_adapter.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "b44cba334e5b94b398263f870b85b26d1b055229",
      "archive": "docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/provider_adapter.rs"
    },
    {
      "old": "packages/ridge-acp/src/spawn.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "d7ef606830022cd216137b43b2f1c375eaac397d",
      "archive": "docs/archive/CHG-070/retired-communion/packages/ridge-acp/src/spawn.rs"
    },
    {
      "old": "packages/ridge-acp/tests/m3_wire.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "b65e8507cb3cbac102e45517580555d2338c77fa",
      "archive": "docs/archive/CHG-070/retired-communion/packages/ridge-acp/tests/m3_wire.rs"
    },
    {
      "old": "src/lib/agent-chat/agentEvents.test.ts",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "b16a7439eb5cf93f7675ec04e9c0fe4fd270c908",
      "archive": "docs/archive/CHG-070/retired-communion/src/lib/agent-chat/agentEvents.test.ts"
    },
    {
      "old": "src/lib/agent-chat/listenerLifecycle.test.ts",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "35d33dfc763fb84e36b0e294c61db551950e0ab1",
      "archive": "docs/archive/CHG-070/retired-communion/src/lib/agent-chat/listenerLifecycle.test.ts"
    },
    {
      "old": "src-tauri/src/bridge/mod.rs",
      "commit": "904729d46ae35a233e1e9f4d8139f957fd149d58",
      "blob": "580e76e302b59b35a7a52de3ca74ab339102c4a7",
      "archive": "docs/archive/CHG-070/retired-communion/src-tauri/src/bridge/mod.rs"
    },
    {
      "old": "static/mobile/assets/index-BPfF1vzx.css",
      "commit": "1c1c6142750369ec7a89184cd6fef0eae061be7b",
      "blob": "59013b29c9ec777765e88ebf6ce3b3a160320bf1",
      "archive": "docs/archive/CHG-090/source/static/mobile/assets/index-BPfF1vzx.css"
    },
    {
      "old": "static/mobile/assets/index-Cm2sAwTC.js",
      "commit": "1c1c6142750369ec7a89184cd6fef0eae061be7b",
      "blob": "9bc80544861ae4d33a89cf0f1591769f15a78406",
      "archive": "docs/archive/CHG-090/source/static/mobile/assets/index-Cm2sAwTC.js"
    },
    {
      "old": "src/lib/remote/cloud/cloudHostTopologyLink.test.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "b729d3d379ccf23208ca01397339d3a3f0828c08",
      "archive": "docs/archive/CHG-070/frontend-cloud-topology/cloudHostTopologyLink.test.ts"
    },
    {
      "old": "src/lib/stores/hostReconnect.test.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "e9a97bd945dd2c1b4d681d793d9073570ee4216f",
      "archive": "docs/archive/CHG-070/hostReconnect.test.ts"
    },
    {
      "old": "src/lib/stores/hostReconnectProduct.test.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "a82dc77c886175e3d77ca2c62b65863ebb59dbd7",
      "archive": "docs/archive/CHG-070/hostReconnectProduct.test.ts"
    },
    {
      "old": "src/lib/stores/hostsOutbound.test.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "c21d6d4331c0cbb293f41993b1c3e89940ae1bb1",
      "archive": "docs/archive/CHG-070/hostsOutbound.test.ts"
    },
    {
      "old": "src/lib/stores/hostsOutboundProduct.test.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "7a2b29d7b36a5b188a3db02d7cd99a71ee6e2f94",
      "archive": "docs/archive/CHG-070/hostsOutboundProduct.test.ts"
    },
    {
      "old": "src/lib/terminal/ptyRuntimeSnapshot.test.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "5e3fd6b83da43b83c4c19d1ce2363bbaa5dcddac",
      "archive": "docs/archive/CHG-090/source/src/lib/terminal/ptyRuntimeSnapshot.test.ts"
    },
    {
      "old": "src/lib/teammate/layoutEvent.test.ts",
      "commit": "3c7a18af4bb88aa3a7131cc45b51624a1fda699a",
      "blob": "8901c650a0b8b8158ec04e1e36e3f7a151ebfeef",
      "archive": "docs/archive/CHG-070/frontend-runtime-events/layoutEvent.test.ts"
    }
  ],
  "nodes": [
    {
      "id": "L4-OBS-STATIC-MOBILE-ASSETS-INDEX-BPFF1VZX-CSS-77c42f58",
      "action": "archived-evidence"
    },
    {
      "id": "L4-OBS-STATIC-MOBILE-ASSETS-INDEX-CM2SAWTC-JS-6cebe785",
      "action": "archived-evidence"
    },
    {
      "id": "L2-REMOTE-RENDERING-PARITY",
      "action": "rebind"
    },
    {
      "id": "L2-TUI-MOBILE-PTY-LIFECYCLE",
      "action": "rebind"
    },
    {
      "id": "L3-PERF-REMOTE-PRODUCTION-DIAGNOSIS",
      "action": "rebind"
    },
    {
      "id": "L3-RIDGE-COMMUNION-M53-ACCEPTANCE",
      "action": "retire"
    },
    {
      "id": "L3-RIDGE-COMMUNION-M53-PRODUCT",
      "action": "retire"
    },
    {
      "id": "L3-RIDGE-COMMUNION-M53-RUNTIME",
      "action": "retire"
    },
    {
      "id": "L3-TERMINAL-VISUAL-INPUT-REPAIR",
      "action": "rebind"
    },
    {
      "id": "L3-OBS-PACKAGES-RIDGE-CLI-SRC-353b0498",
      "action": "retain"
    },
    {
      "id": "L3-OBS-PACKAGES-RIDGE-CLI-SRC-TUI-985fdbf4",
      "action": "retain"
    },
    {
      "id": "L3-OBS-SRC-LIB-REMOTE-CLOUD-643692b0",
      "action": "retain"
    },
    {
      "id": "L3-OBS-SRC-LIB-STORES-c2ef71cf",
      "action": "retain"
    },
    {
      "id": "L3-OBS-SRC-LIB-TEAMMATE-8177ace9",
      "action": "retain"
    },
    {
      "id": "L3-OBS-SRC-LIB-TERMINAL-033d05be",
      "action": "retain"
    },
    {
      "id": "L3-OBS-SRC-TAURI-SRC-bcb33161",
      "action": "retain"
    },
    {
      "id": "L3-OBS-SRC-TAURI-SRC-COMMANDS-7ed73efa",
      "action": "retain"
    },
    {
      "id": "L3-OBS-SRC-TAURI-SRC-DB-fb3d3b31",
      "action": "retire"
    },
    {
      "id": "L3-OBS-SRC-TAURI-SRC-ENGINE-7710ea45",
      "action": "retire"
    },
    {
      "id": "L3-OBS-SRC-TAURI-SRC-FS-85347803",
      "action": "retire"
    },
    {
      "id": "L3-OBS-SRC-TAURI-SRC-HOSTS-728bec8b",
      "action": "retire"
    },
    {
      "id": "L3-OBS-SRC-TAURI-SRC-LSP-baee4624",
      "action": "retire"
    },
    {
      "id": "L3-OBS-SRC-TAURI-SRC-TEAMMATE-cbffd630",
      "action": "retire"
    },
    {
      "id": "L3-OBS-SRC-TAURI-SRC-UTILS-679c9ad4",
      "action": "retain"
    },
    {
      "id": "L4-OBS-DRIFT-BASELINE-001-UNCLAIMED-a1b2c3d4",
      "action": "retain"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-ACP-SRC-COMMUNION-RS-M41",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-ACP-SRC-PROVIDER-ADAPTER-RS",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-ACP-SRC-SPAWN-RS-M3",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-ACP-TESTS-M3-WIRE-RS",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-CLI-SRC-DAEMON-CTL-RS-487827aa",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-CLI-SRC-SESSION-RS-1eb4fe21",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-CLI-SRC-TUI-LAN-HOST-IMPL-RS-a0422c05",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-CLI-SRC-TUI-LAN-HOST-RS-dee73956",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-CLI-SRC-TUI-SCROLLBACK-RS-e98d3d15",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-CLI-SRC-TUI-WORKSPACE-RS-2ac7d76c",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-AGENT-CHAT-AGENT-EVENTS-TEST-TS",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-AGENT-CHAT-AGENT-EVENTS-TS-M41",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-AGENT-CHAT-DEFAULT-WIRED-SVELTE-M3",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-AGENT-CHAT-LISTENER-LIFECYCLE-TEST-TS-M42",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-AGENT-CHAT-PICKER-SVELTE",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-AGENT-CHAT-PROVIDER-RESTORE-TS-M42",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTTOPOLOGYLINK-TS-680544f6",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-HOSTRECONNECT-TS-f82d2a23",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-LAYOUTCHANGE-GOLDEN-JSON-148c3d54",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-LAYOUTEVENT-TS-da106aca",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-LIB-TERMINAL-PTYRUNTIMESNAPSHOT-TS-c8caed0f",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-BRIDGE-ACP-BRIDGE-RS-a1b2c3d4",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-BRIDGE-MOD-RS-b1c2d3e4",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-CLOUD-HTTP-RS-298f4282",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-CLOUD-PANE-RS-65fe7524",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-FS-WATCH-RS-154fe1c6",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-GIT-RS-8be46cbc",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-PANE-RS-083d2191",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-PROCESS-RS-bd4bf5d4",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-REMOTE-RS-fb28910e",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-SETTINGS-RS-b743c5a2",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-TEAMMATE-RS-9292e20f",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-TERMINAL-FONT-RS-d1768053",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-TERMINAL-RS-12565300",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-WATCH-RS-5a7c9f05",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-COMMANDS-WORKSPACE-RS-8a3dbc5b",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-DB-MOD-RS-ddc5c908",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-DB-PROJECTS-RS-5c0fcc7d",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-ENGINE-KERNEL-PTY-RS-c387fe33",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-ENGINE-MOD-RS-24e36859",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-ENGINE-PANE-TREE-RS-81275760",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-ENGINE-PARSER-RS-a790a763",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-ENGINE-PTY-RS-104cc347",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-FS-MOD-RS-ed16b557",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-FS-SEARCH-RS-382fd4a8",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-FS-TREE-RS-0009918d",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-HOSTS-DESKTOP-SURFACE-RS-5d6068b4",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-HOSTS-FOREIGN-HISTORY-RS-857e8b18",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-HOSTS-HISTORY-COMMANDS-RS-977691ec",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-HOSTS-LAN-TRANSPORT-RS-719e4239",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-HOSTS-LIVE-BACKPRESSURE-RS-f1ca296e",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-HOSTS-MOD-RS-eaff247c",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-HOSTS-OUTBOUND-RS-b2497a1b",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-HOSTS-RECONNECT-SUPERVISOR-RS-648b14f2",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-LSP-MOD-RS-4d2b0b61",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-REMOTE-BRIDGE-RS-13885a6e",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-REMOTE-HOST-IMPL-RS-5d15687d",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-AGENT-CATALOG-RS-b8be1b61",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-AUTODISCOVER-RS-46b64992",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-CIRCUIT-RS-c11883e9",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-CONTEXT-FILES-RS-8b3b23e0",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-DISCOVER-RS-07f49d8f",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-ENDPOINT-RS-ba22be5c",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-HITL-AUDIT-RS-5a6322dd",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-HITL-RS-467f3dcb",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-JOB-OBJECT-RS-9da011ec",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-LAYOUT-EVENT-RS-5ec95309",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-LOCKS-RS-11ac3f59",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-MCP-RS-f1741d1a",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-MEMORY-RS-05ee0e18",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-MOD-RS-9ac71a21",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-NATIVE-RS-f38e554c",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-ORCH-HEALTH-RS-77c04f81",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-OS-FREEZE-RS-bf5d5bc1",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-PROFILES-RS-da746222",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-ROLLBACK-RS-1f400db9",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-SERVER-RS-20eb0315",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TEAMMATE-SUSPEND-RS-169abdf1",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-TYPES-RS-5a17982b",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-UTILS-ERROR-RS-91706d06",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-UTILS-PANE-ID-RS-c5e7302c",
      "action": "retire"
    },
    {
      "id": "L4-OBS-SRC-TAURI-SRC-UTILS-PTY-LOG-RS-cfe4a773",
      "action": "retire"
    },
    {
      "id": "L2-RIDGE-COMMUNION-M53",
      "action": "retire"
    },
    {
      "id": "TEST-OBS-SRC-LIB-REMOTE-REMOTEBOOTMODE-TEST-TS-0c3742d1",
      "action": "reparent"
    },
    {
      "id": "TEST-OBS-SRC-LIB-TEAMMATE-AGENTCOMMUNEMODEL-TEST-TS-313d1942",
      "action": "reparent"
    },
    {
      "id": "TEST-OBS-SRC-LIB-TEAMMATE-TEAMMATEMODEL-TEST-TS-99fea951",
      "action": "reparent"
    },
    {
      "id": "L3-OBS-STATIC-MOBILE-ASSETS-5cc42e7a",
      "action": "retire"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-CLI-SRC-HOST-RS-435d6ebb",
      "action": "retain"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-CLI-SRC-PTY-RS-c81f693e",
      "action": "retain"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-CORE-SRC-TEAMMATE-TOPOLOGY-RS-ec181c11",
      "action": "retain"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-KERNEL-SRC-PTY-RS-7a2cf18e",
      "action": "retain"
    },
    {
      "id": "L4-OBS-PACKAGES-RIDGE-REMOTE-SRC-HOST-RS-b5c8faed",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-APP-CSS-e269447b",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-APP-HTML-c279397c",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-HOOKS-SERVER-TS-b5aa13ce",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-DEVISSUE-TS-930b4e3d",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-CLOUD-CHECKINGATECARD-SVELTE-7b71a8aa",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-TS-3ba527ac",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTSTORE-TS-86f442a0",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDPROMODAL-SVELTE-7600402d",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-TS-27c07ba9",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-MINIMIZEBUTTON-SVELTE-ec260d1a",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-QRCODE-D-TS-5999ec56",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-QRCODE-SVELTE-dbe7af3c",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-REMOTEBOOTMODE-TS-8152e7b4",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-REMOTEPANEL-SVELTE-666f7d6c",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-TOTPIDENTITYSYNC-TS-db20536e",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-REMOTE-TREENODEROW-SVELTE-f3c6ad58",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-CONTEXTMENU-TS-08b90d28",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-DOCKREGIONPICKER-TS-6a37cd47",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-EDITORWINDOW-TS-da005f99",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-EXPLORERLAYOUT-TS-7945c5db",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-FILEEDITOR-TS-301d7e43",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-FILEEXPLORER-TS-e2a5ccec",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-FILEWATCHERSYNC-TS-c625380b",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-FSEVENTS-TS-93e667ef",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-GITGUARDSTATS-TS-50279592",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-HOSTS-TS-74da5b22",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-IMAGEPREVIEWVERSION-TS-f845c21b",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-PANEGITSTATUS-TS-6d620300",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-PANETREE-TS-deea5908",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-PROCESSGUARDPOLICY-TS-6851e66a",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-PROJECT-TS-705c863b",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-REMOTESTATUS-TS-4532b75b",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-SCMCACHE-TS-f1f5bdda",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-SEARCHSTATE-TS-92db44fe",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-SETTINGS-TS-5bc47293",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-SIDEBARPLUGINS-TS-949b8758",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-TERMINALHISTORY-TS-572f35e8",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-TERMSETTINGS-TS-dd19d16f",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-THEMES-TS-8aec2ce9",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-STORES-TOAST-TS-553cfde2",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-AGENTCENTERPANEL-SVELTE-c54ca71e",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-AGENTCOMMUNEMODEL-TS-2b0afbe5",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-AGENTMEMBERROW-SVELTE-9d24ed62",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-AGENTPANEHIGHLIGHTSYNC-SVELTE-31e31ba4",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-AGENTPANEHIGHLIGHTSYNC-TS-b5723d1e",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-HITLAPPROVALMODAL-SVELTE-f8a87f58",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-HITLAUDITFILTER-TS-3c7fa406",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-HITLAUDITPANEL-TS-42d4382f",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-MEMBERTASKS-TS-286be00f",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-ORCHCONTROLPLANE-TS-abcb9735",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-TEAMMATEGROUPS-SVELTE-TS-81a642bc",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-TEAMMATEGROUPSSECTION-SVELTE-53ade9d1",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-TEAMMATEMODEL-TS-5b42791e",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-TEAMMATESETTINGS-TS-212d29ba",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TEAMMATE-WORKSPACEMEMORY-TS-f12c4006",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TERMINAL-DESKTOPPANERESIZE-TS-cd4234d3",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TERMINAL-HOSTPORTS-TS-86368fac",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TERMINAL-PANESIZESYNC-TS-9f63f52d",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TERMINAL-PTYWRITEQUEUE-TS-0440e203",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TRANSPORT-TAURISHIM-EVENT-TS-feb0138e",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-LIB-TYPES-TS-b8b77f5b",
      "action": "retain"
    },
    {
      "id": "L4-OBS-SRC-SERVICE-WORKER-TS-0aba05b7",
      "action": "retain"
    },
    {
      "id": "TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTTOPOLOGYLINK-TEST-TS-7c92f09e",
      "action": "retire"
    },
    {
      "id": "TEST-OBS-SRC-LIB-STORES-HOSTRECONNECT-TEST-TS-e82e61be",
      "action": "retire"
    },
    {
      "id": "TEST-OBS-SRC-LIB-STORES-HOSTRECONNECTPRODUCT-TEST-TS-9598a61c",
      "action": "retire"
    },
    {
      "id": "TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUND-TEST-TS-afc6c610",
      "action": "retire"
    },
    {
      "id": "TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUNDPRODUCT-TEST-TS-c9cf71df",
      "action": "retire"
    },
    {
      "id": "TEST-OBS-SRC-LIB-TEAMMATE-LAYOUTEVENT-TEST-TS-b130fa00",
      "action": "retire"
    },
    {
      "id": "TEST-OBS-SRC-LIB-TERMINAL-PTYRUNTIMESNAPSHOT-TEST-TS-a223e946",
      "action": "retire"
    },
    {
      "id": "L4-OBS-DOTENV-d1e2f3a4",
      "action": "reparent"
    }
  ]
}
```

归档子树用局部 .gitattributes 禁止换行转换，保留原始 Git blob 的 CRLF/LF 字节。此规则只作用本轮历史证据 source/。

活动子节点不能留在本轮退役父节点下。根环境观察记录从旧生成资产目录迁至 Vite 构建配置模块，仅修正元数据归属。
