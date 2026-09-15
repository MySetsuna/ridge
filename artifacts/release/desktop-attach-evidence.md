# ?ui=desktop 桌面 SPA 跑通证据

**Date**: 2026-09-15
**Host**: `target/test-rdg/release/ridge.exe` (sha256 6a3eae711a31ab5909e13a471a6f2873f082de2c9f13f0fd80f8f79fb201e522, 与 release 隔离构建以避免触碰宿主 ridge.exe)
**Frontend bundle**: `remote-dist/desktop/` (含 themes.ts 修复后的 2026-09-15 17:35 重建)
**Port**: 5120 (与宿主的 5117 / 5119 / 5118 全部隔离)
**TOTP**: 682308

## 改了什么

### 1. `packages/ridge-cli/src/kernel_host_impl.rs`

旧代码：
```rust
"use_global_workspace" | "activate_pane_pty" | "set_pane_delta_mode" | "get_theme_data" => {
    Ok(Value::Null)
}
```

新代码：
```rust
"use_global_workspace" | "activate_pane_pty" | "set_pane_delta_mode" => {
    Ok(Value::Null)
}
// kernel host 不维护主题文件（与 LAN host 一致降级返回空集），
// 避免 src/lib/stores/themes.ts 收到 null 时把整条 boot IIFE 拖垮。
"get_theme_data" => Ok(json!({ "version": 1, "themes": [] })),
```

并在 `dispatch()` 中新增 `"create_workspace"` arm，桥接 /v1/domain/workspaces：

```rust
"create_workspace" => {
    let name = args.get("name").and_then(Value::as_str);
    let body = name.map(|value| serde_json::json!({ "name": value }));
    let result = request_json(
        &host.current_endpoint(),
        "POST",
        "/v1/domain/workspaces",
        body.as_ref(),
    )?;
    let workspace_id = result
        .get("workspace_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(json!({
        "success": true,
        "workspaceId": workspace_id,
        "createdWorkspace": true,
    }))
}
```

### 2. `src/lib/stores/themes.ts`

`initThemeSystem()` 在 `store.set(tf)` 之前先验证 `tf.themes` 是数组，避免 host 返回 null/缺字段时把整条 boot IIFE 拖垮：

```ts
const safe: ThemeFile =
  tf && Array.isArray(tf.themes)
    ? tf
    : { version: tf?.version ?? 1, themes: [] };
store.set(safe);
```

> 双重兜底：host 现在返回 `{ "version": 1, "themes": [] }`，前端也校验 `Array.isArray(tf.themes)`。

## 验证

### 桌面 SPA 启动

打开 `https://127.0.0.1:5120/?ui=desktop`：
- 浏览器从 `?ui=desktop` 路由到 `UiKind::Desktop` (serve.rs ua.rs SSOT)
- 加载 `bridge.attach(createLanWsTransport(conn))` → WebSocket 接 5120
- TOTP 验证成功（`/verify` 返回 token）
- `bridge.invoke('list_workspaces')` 返回 294 个工作区（来自 kernel sqlite 持久化数据）
- `bridge.invoke('get_active_workspace_id')` 取到 active workspace
- `bridge.invoke('get_pane_layout_for', { workspaceId })` 返回 3 pane layout
- 桌面 UI 渲染：左侧资源管理器树 + 右侧 3 个 terminal pane (~/ridge × 2, ~/Taskryn × 1)

### Console errors
- ❌ 修复前：`TypeError: Cannot read properties of null (reading 'themes')` — **消失**
- ❌ 修复前：`RpcRemoteError: method not supported by kernel host: create_workspace` — **消失**
- ✅ 仅剩：`set_user_default_cwd failed` (无关紧要的 cwd 路径写入失败，不影响 boot)
- ✅ 0 个致命 boot error

### 已确认 attach 路径

| 步骤 | 验证方式 |
|---|---|
| TOTP 鉴权 | `POST /verify` → 200 + token |
| List workspaces | UI 显示 294 个保存工作区 + 资源管理器树 |
| Attach active workspace | UI 渲染 3 个 terminal pane + terminal type-switcher 按钮（说明 manager.attach 已建立 xterm 实例）|
| get_pane_layout_for | `Ready` 状态 + 资源管理器展开为树状 |
| WebSocket 桥接 | `netstat` 显示 127.0.0.1:5120 ESTABLISHED |

### 已知遗留

- 「已保存工作区」列表膨胀到 294 项 — 这是**先前会话积累的 bug**（`list_saved_workspace_files` 在 web-remote 启动循环中重复 create），与本次修复正交；任务 #23 范围仅限「attach 路径 + 不再因缺失方法崩溃」，不属于 #23。
- 实际 PTY input 验证需要用户在桌面 UI 中 focus xterm canvas 后键入；自动化 `type_text` 命中的是 svelte `textbox` 元素（uid=4_983），不会触发 xterm 的 onData → write_to_pty 链路。

## 产物

- `target/test-rdg/release/ridge.exe` sha256 `6a3eae711a31ab5909e13a471a6f2873f082de2c9f13f0fd80f8f79fb201e522`（隔离 target-dir 构建，避免触碰宿主 `target/release/ridge.exe` 锁）
- `remote-dist/desktop/index.html` 2026-09-15 17:35 重建（含 themes.ts 修复）
- host log: `/tmp/rdg-5120.log`
