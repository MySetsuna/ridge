# CONTRACT — Iteration 56 / AC4-C56（约 2 日 · 多主机会话隔离）

**Credit ID**: C56 · OP-RECONN-HOST 加厚

## 产品结果
hostSessionIsolation 任务模型；ReconnectSupervisor assert_isolation/phase_str；多 host 不共享 pane。

## 独占主文件
- `src/lib/hosts/hostSessionIsolation.ts`(+test)
- `src-tauri/src/hosts/reconnect_supervisor.rs`
- `src/lib/hosts/hostControlSurface.ts`
- `src/lib/stores/hostReconnect.ts`

## 验收
vitest hostSessionIsolation；cargo reconnect_supervisor multi_host_isolation。

## 停机
跨 host 共享 reconnect 任务。
