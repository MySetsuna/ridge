---
id: L4-OBS-SRC-LIB-TEAMMATE-TEAMMATESETTINGS-TS-212d29ba
level: L4
parent: L3-OBS-SRC-LIB-TEAMMATE-8177ace9
title: teammateSettings.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/teammate/teammateSettings.ts
test_targets:
  - src/lib/teammate/agentCommuneModel.test.ts
  - src/lib/teammate/agentPaneHighlightSync.test.ts
  - src/lib/teammate/hitlAuditFilter.test.ts
  - src/lib/teammate/hitlAuditPanel.test.ts
  - src/lib/teammate/memberTasks.test.ts
  - src/lib/teammate/orchControlPlane.test.ts
  - src/lib/teammate/teammateGroups.test.ts
  - src/lib/teammate/teammateModel.test.ts
  - src/lib/teammate/workspaceMemory.test.ts
public_interface:
  - "export function initTeammateBoot(): () => void"
  - "export function setTeammateEnabled(enabled: boolean): void"
  - "export function setTeammateHitlEnabled(enabled: boolean): Promise<void>"
  - "export function syncTeammateBackend(s: UserSettings = get(settingsStore)"
verified_by:
  - TEST-OBS-SRC-LIB-TEAMMATE-AGENTCOMMUNEMODEL-TEST-TS-313d1942
  - TEST-OBS-SRC-LIB-TEAMMATE-AGENTPANEHIGHLIGHTSYNC-TEST-TS-68715b97
  - TEST-OBS-SRC-LIB-TEAMMATE-HITLAUDITFILTER-TEST-TS-1bebca23
  - TEST-OBS-SRC-LIB-TEAMMATE-HITLAUDITPANEL-TEST-TS-44e3a8b8
  - TEST-OBS-SRC-LIB-TEAMMATE-LAYOUTEVENT-TEST-TS-b130fa00
  - TEST-OBS-SRC-LIB-TEAMMATE-MEMBERTASKS-TEST-TS-0d803223
  - TEST-OBS-SRC-LIB-TEAMMATE-ORCHCONTROLPLANE-TEST-TS-71f485a7
  - TEST-OBS-SRC-LIB-TEAMMATE-TEAMMATEGROUPS-TEST-TS-e7ea1fa2
  - TEST-OBS-SRC-LIB-TEAMMATE-TEAMMATEMODEL-TEST-TS-99fea951
  - TEST-OBS-SRC-LIB-TEAMMATE-WORKSPACEMEMORY-TEST-TS-ac0bbd3e
---

# teammateSettings.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置 — 测试引用

旧测试已随 CHG-070 源链迁出。保留原始测试映射及 Git 归档证据，移除现役失效引用。其余原有测试目标保留，不按名称猜测替代覆盖。

原始 test_targets

```json
[
  "src/lib/teammate/agentCommuneModel.test.ts",
  "src/lib/teammate/agentPaneHighlightSync.test.ts",
  "src/lib/teammate/hitlAuditFilter.test.ts",
  "src/lib/teammate/hitlAuditPanel.test.ts",
  "src/lib/teammate/layoutEvent.test.ts",
  "src/lib/teammate/memberTasks.test.ts",
  "src/lib/teammate/orchControlPlane.test.ts",
  "src/lib/teammate/teammateGroups.test.ts",
  "src/lib/teammate/teammateModel.test.ts",
  "src/lib/teammate/workspaceMemory.test.ts"
]
```

`src/lib/teammate/layoutEvent.test.ts` → `docs/archive/CHG-070/frontend-runtime-events/layoutEvent.test.ts`

## CHG-090 映射处置 — 导出快照

Commune 启动监听提供清理函数，审批设置更新返回异步结果。 仅更新迁移观察的确定性导出快照，产品要求保留。按 SpecTree SOP 执行 interface L4-OBS-SRC-LIB-TEAMMATE-TEAMMATESETTINGS-TS-212d29ba --confirm 后重新批准和锁定。

旧声明保留如下。

```json
[
  "export function initTeammateBoot(): void",
  "export function setTeammateEnabled(enabled: boolean): void",
  "export function setTeammateHitlEnabled(enabled: boolean): void",
  "export function syncTeammateBackend(s: UserSettings = get(settingsStore)"
]
```
