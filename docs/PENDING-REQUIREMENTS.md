# Ridge · 待审批需求

> 本文件仅存本地审批界面，不上传 NotebookLM，不构成实施合同。

## 待审批变更 (Pending Changes)

### PENDING-REQ-20260805-AGENT-COMMUNICATION-REGISTRY-01 · Deterministic Agent lifecycle and communication registry

- Type: `NEW`
- Original intent: Maintain one authoritative place to register Agents when creation and destruction are confirmed, and require every Agent to inspect the current communication directory before sending messages so it knows valid targets and online state. Failed creation/destruction attempts must not become active communication records.
- Related Active requirement: `REQ-RIDGE-KERNEL-DOMAIN-01`, `REQ-RIDGE-KERNEL-HOST-01`, `REQ-AGENT-CATALOG-01`, `REQ-AGENT-COMMUNE-UI-02`, and `REQ-RIDGE-MCP-AS-KERNEL-API-01`.
- Target behavior: The kernel/teammate authority owns a stable `agent_id` record containing session, workspace, pane, CWD, lifecycle generation/lease, status, online state, and last-seen time. A create record is committed only after spawn/attach success; a destroy record is committed only after destroy/lease closure success. Failed or partial attempts stay diagnostic-only and never enter the active directory. Before communication, the sender takes a fresh bounded roster snapshot, validates the target identity plus current generation/lease and online state, performs at most one refresh when stale, then sends once or returns a typed missing/offline result.
- Scope: Kernel Agent/teammate registry and lifecycle APIs; ridge CLI/MCP adapters; desktop and Remote Agent/Commune projections; roster lookup, online/lease validation, idempotency/single-flight, disconnect cleanup, and deterministic unit/integration/multi-Agent tests.
- Non-goals: No second UI-local authoritative directory; no identity inferred from title or CWD; no silent respawn; no unbounded retry; no changes to the Remote wire protocol before the registry contract is approved.
- Frozen boundary: Existing PTY, pane, workspace-singleton, Remote, and MCP transport contracts remain authoritative; this request adds lifecycle/communication truth and adapters only after approval.
- Assumptions/open questions: Confirm whether an Agent destroy failure remains an online-but-unresolved lease or enters an explicit `destroy_pending` state; define roster snapshot TTL and generation invalidation event; identify the exact kernel endpoint/name used by existing teammate orchestration.
- Deterministic acceptance: Successful create appears exactly once; failed create leaves no active entry; successful destroy removes or closes exactly one generation; failed destroy is visible and blocks unsafe reuse; stale/offline target is rejected before send; one bounded refresh cannot spawn duplicates; concurrent sends use one idempotency key and no duplicate communication; reconnect generation races cannot deliver to an old Agent; teardown cancels pending calls; at least one real/equivalent multi-Agent E2E proves CWD/session-independent discovery.
- Expected traceability: `PENDING-REQ-20260805-AGENT-COMMUNICATION-REGISTRY-01` → kernel roster/lifecycle contract → CLI/MCP and desktop/Remote adapters → deterministic tests and multi-Agent E2E → iteration archive.
