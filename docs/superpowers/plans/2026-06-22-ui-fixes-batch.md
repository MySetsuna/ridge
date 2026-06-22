# UI 缺陷批量修复 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 5 个桌面端 UI 缺陷——开关圆钮溢出、工作区 `+` 按钮间距、每个 pane 切换终端类型的 per-pane 状态、终端类型检测补全（WSL 发行版/VS 开发者环境）、拖拽 pane 头部停靠失效。

**Architecture:** SvelteKit 前端（`src/`，Svelte 5 runes）+ Tauri/Rust 后端（`src-tauri/`）+ 纯逻辑下沉 `packages/ridge-core`。窗口在 `src-tauri/src/lib.rs:191` 编程式创建。拖拽停靠改用指针事件以绕开 Tauri `drag_drop_enabled=true` 在 WebView2 上对 HTML5 DnD 的屏蔽（同时保留 OS 文件拖放）。

**Tech Stack:** Svelte 5 (`$props`/`$state`/`$derived`)、Tailwind v4、Tauri v2、Rust（portable-pty）、vitest（前端单测）、cargo test（ridge-core）。

## Global Constraints

- **沟通/注释**：思考用英文、回复用中文；代码注释沿用各仓库既有语言风格（本仓库注释多为中文）。
- **提交粒度**：每个 Task 单独 commit；commit message 末尾加 `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`。当前分支 `develop`，直接在其上提交（与仓库既有惯例一致）。
- **Rust 验证约束**：本机常驻 `tauri dev` 自动重建——**不要并行跑 `cargo check`/`cargo build`**（抢 target 锁）。桌面 crate（`ridge`）的改动靠常驻 dev 重建是否通过来验证；其测试 exe 因 comctl32 v6 清单缺失无法独立启动，**不给桌面 crate 加单测**。可单测的纯逻辑下沉 `ridge-core`，用 `cargo test -p ridge-core`（必要时先暂停 dev 再跑，避免锁冲突）。
- **前端验证**：类型用 `pnpm check`；单测用 `pnpm test`（vitest）。
- **不碰**：`src-tauri/Cargo.toml` 已有的本地修改保持不动；FileTree 的 HTML5 DnD 不在本批次范围。

---

### Task 1: 工作区 `+` 按钮与 tab 间距（②）

**Files:**
- Modify: `src/routes/+page.svelte`（`{#snippet trailingActions()}` 内的 `+` 按钮，约 `:1682-1689`）

**Interfaces:**
- Consumes: 无
- Produces: 无（纯样式）

- [ ] **Step 1: 给 `+` 按钮加左间距**

在 `src/routes/+page.svelte` 找到 trailingActions 的 `+` 按钮，其 class 当前以 `shrink-0 flex h-8 w-8 items-center justify-center rounded-lg border border-dashed ...` 开头。把开头的 `shrink-0` 改成 `shrink-0 ml-2`：

```svelte
{#snippet trailingActions()}
  <button
    type="button"
    class="shrink-0 ml-2 flex h-8 w-8 items-center justify-center rounded-lg border border-dashed border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:border-[var(--rg-accent)]/40 hover:text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/8 transition-colors"
    title={$t('main.newWorkspaceBtn')}
    onclick={() => createWorkspace()}
  >
    <span class="text-lg leading-none">+</span>
  </button>
{/snippet}
```

- [ ] **Step 2: 类型检查**

Run: `pnpm check`
Expected: PASS（0 errors；既有 warning 不计）

- [ ] **Step 3: Commit**

```bash
git add src/routes/+page.svelte
git commit -m "fix(workspace): + 按钮与左侧 tab 留出间距"
```

---

### Task 2: 抽取可复用 `Toggle.svelte` 并修远程控制开关（①）

**Files:**
- Create: `src/lib/components/Toggle.svelte`
- Modify: `src/lib/components/SettingsPanel.svelte`（import 区 + 远程控制开关 `:439-467`）

**Interfaces:**
- Produces: `Toggle.svelte`，props `{ checked: boolean; onchange: (next: boolean) => void; disabled?: boolean; ariaLabel?: string; title?: string }`

- [ ] **Step 1: 新建 `Toggle.svelte`**

几何要点：轨道 `relative` + 圆钮 `absolute` 用 `left` 定位（off → `left-0.5`；on → `left-[calc(100%-1.125rem)]`，其中 `100%` 取自轨道 padding box、`1.125rem`=圆钮 1rem + 0.125rem 间隙），垂直 `top-1/2 -translate-y-1/2`。**不用固定 px 的 `translate-x`**，任意根字号/缩放下圆钮恒在轨道内、右侧留 2px。

Create `src/lib/components/Toggle.svelte`:

```svelte
<script lang="ts">
  // 可复用开关：rem 自洽几何，圆钮用 left(% + rem) 定位，数学上不会溢出轨道。
  interface Props {
    checked: boolean;
    onchange: (next: boolean) => void;
    disabled?: boolean;
    ariaLabel?: string;
    title?: string;
  }
  let { checked, onchange, disabled = false, ariaLabel, title }: Props = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={ariaLabel}
  {title}
  {disabled}
  onclick={() => onchange(!checked)}
  class="relative inline-flex shrink-0 h-5 w-9 rounded-full border transition-colors disabled:opacity-50 disabled:cursor-not-allowed {checked
    ? 'bg-[var(--rg-accent)] border-[var(--rg-accent)]'
    : 'bg-[var(--rg-surface-2)] border-[var(--rg-border)]'}"
>
  <span
    class="pointer-events-none absolute top-1/2 -translate-y-1/2 h-4 w-4 rounded-full bg-white shadow-sm transition-[left] duration-150 {checked
      ? 'left-[calc(100%-1.125rem)]'
      : 'left-0.5'}"
  ></span>
</button>
```

- [ ] **Step 2: 在 SettingsPanel 引入并替换**

在 `SettingsPanel.svelte` 的 import 区（`import LangSwitch from './LangSwitch.svelte';` 附近）加：

```svelte
import Toggle from './Toggle.svelte';
```

把 `activeSection === 'extensions'` 分支里的 `<button role="switch" ...> ... </button>`（含其内 `<span>` 圆钮，整段 `:439-467`）替换为：

```svelte
<Toggle
  checked={$settingsStore.remoteEnabled}
  ariaLabel={$t('settings.remoteToggle')}
  title={$settingsStore.remoteEnabled ? $t('settings.remoteToggleOn') : $t('settings.remoteToggleOff')}
  onchange={async (next) => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('set_remote_enabled', { enabled: next });
    } catch (e) {
      console.warn($t('settings.remoteToggleFailed'), e);
      void refreshRemoteRunning();
      return;
    }
    setSetting('remoteEnabled', next);
    void refreshRemoteRunning();
  }}
/>
```

- [ ] **Step 3: 类型检查**

Run: `pnpm check`
Expected: PASS

- [ ] **Step 4: 运行时目测**

在常驻 dev 里打开 设置→扩展，确认开关圆钮在开/关两态都完整落在轨道内、不溢出；点击可切换远程控制。

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/Toggle.svelte src/lib/components/SettingsPanel.svelte
git commit -m "feat(ui): 抽取可复用 Toggle 组件，修开关圆钮溢出"
```

> 备注：用户提到的"智能体设置"开关不在当前 develop 分支（该面板在建）。本 Task 只交付组件 + 修远程控制处；该面板落地后套用同 `<Toggle>` 即可。

---

### Task 3: 终端类型检测补全（④，ridge-core 纯逻辑 + 测试）

**Files:**
- Modify: `packages/ridge-core/src/commands/shell.rs`（`ShellInfo` 加 `args`；Windows 分支加 WSL 发行版 + VS 枚举；新增纯解析函数 + 测试模块）

**Interfaces:**
- Produces: `ShellInfo { id: String, label: String, program: String, args: Vec<String> }`（`args` 带 `#[serde(default)]`）；`fn parse_wsl_list(stdout: &[u8]) -> Vec<String>`（纯函数，可测）

- [ ] **Step 1: 先写失败测试（WSL 列表解析）**

在 `packages/ridge-core/src/commands/shell.rs` 末尾追加测试模块（`parse_wsl_list` 解码 `wsl -l -q` 的 UTF-16LE 输出）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn parse_wsl_list_decodes_utf16le_and_trims() {
        // `wsl -l -q` 输出 UTF-16LE，每行一个发行版，可能带 CR / 尾随空行。
        let bytes = utf16le("Ubuntu\r\nDebian\r\n\r\n");
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu".to_string(), "Debian".to_string()]);
    }

    #[test]
    fn parse_wsl_list_empty_is_empty() {
        assert_eq!(parse_wsl_list(&utf16le("")), Vec::<String>::new());
    }

    #[test]
    fn parse_wsl_list_strips_nul_padding() {
        // 某些环境会夹带 NUL；不应产生空条目。
        let mut bytes = utf16le("Ubuntu");
        bytes.extend_from_slice(&[0, 0]); // trailing NUL u16
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu".to_string()]);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p ridge-core parse_wsl_list`
Expected: FAIL（`parse_wsl_list` 未定义，编译错误）

- [ ] **Step 3: `ShellInfo` 加 `args` 字段**

把 `ShellInfo` 结构（`:15-20`）改为：

```rust
#[derive(serde::Serialize)]
pub struct ShellInfo {
    pub id: String,
    pub label: String,
    pub program: String,
    /// 启动参数（如 WSL 发行版 `["-d","Ubuntu"]`、VS `["/k", "...VsDevCmd.bat"]`）。
    /// 空表示直接启动 program。`#[serde(default)]` 兼容旧反序列化路径。
    #[serde(default)]
    pub args: Vec<String>,
}
```

并把现有 `try_add` 闭包里 `list.push(ShellInfo { id, label, program: prog })` 改为带 `args: vec![]`：

```rust
list.push(ShellInfo {
    id: id.to_string(),
    label: label.to_string(),
    program: prog,
    args: vec![],
});
```

- [ ] **Step 4: 实现 `parse_wsl_list` + WSL 发行版枚举 + VS 枚举**

在 `detect_available_shells` 之上（或之下）新增纯解析函数：

```rust
/// 解析 `wsl.exe -l -q` 的 stdout（UTF-16LE）为发行版名列表，去掉空行 / CR / NUL。
pub fn parse_wsl_list(stdout: &[u8]) -> Vec<String> {
    let u16s: Vec<u16> = stdout
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&u16s)
        .lines()
        .map(|l| l.trim().trim_matches('\0').trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}
```

Windows 专用枚举助手（放在 `#[cfg(target_os = "windows")]` 区域附近，整个 fn 加 `#[cfg(target_os = "windows")]`）：

```rust
#[cfg(target_os = "windows")]
fn list_wsl_distros() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    match Command::new("wsl.exe")
        .args(["-l", "-q"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) if o.status.success() => parse_wsl_list(&o.stdout),
        _ => Vec::new(),
    }
}

#[cfg(target_os = "windows")]
fn detect_vs_dev_shells() -> Vec<ShellInfo> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut out = Vec::new();
    let pf86 = match std::env::var("ProgramFiles(x86)") {
        Ok(p) if !p.is_empty() => p,
        _ => return out,
    };
    let vswhere = PathBuf::from(&pf86)
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if !vswhere.is_file() {
        return out;
    }
    let install_path = match Command::new(&vswhere)
        .args(["-latest", "-property", "installationPath"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return out,
    };
    if install_path.is_empty() {
        return out;
    }
    let install = PathBuf::from(&install_path);

    // Developer Command Prompt：cmd /k VsDevCmd.bat
    let vsdevcmd = install.join("Common7").join("Tools").join("VsDevCmd.bat");
    if vsdevcmd.is_file() {
        if let Some(cmd) = lookup_program("cmd.exe") {
            out.push(ShellInfo {
                id: "vs-devcmd".to_string(),
                label: "Developer Command Prompt for VS".to_string(),
                program: cmd.to_string_lossy().to_string(),
                args: vec!["/k".to_string(), vsdevcmd.to_string_lossy().to_string()],
            });
        }
    }

    // Developer PowerShell：powershell -NoExit -Command "Import-Module DevShell.dll; Enter-VsDevShell -VsInstallPath ..."
    let devshell = install
        .join("Common7")
        .join("Tools")
        .join("Microsoft.VisualStudio.DevShell.dll");
    if devshell.is_file() {
        if let Some(ps) = lookup_program("powershell.exe") {
            let script = format!(
                "Import-Module '{}'; Enter-VsDevShell -VsInstallPath '{}' -SkipAutomaticLocation",
                devshell.to_string_lossy(),
                install.to_string_lossy()
            );
            out.push(ShellInfo {
                id: "vs-pwsh".to_string(),
                label: "Developer PowerShell for VS".to_string(),
                program: ps.to_string_lossy().to_string(),
                args: vec!["-NoExit".to_string(), "-Command".to_string(), script],
            });
        }
    }
    out
}
```

在 `detect_available_shells` 的 `#[cfg(target_os = "windows")]` 块里，把原来的单条 `try_add(&mut found, "wsl", "WSL (Ubuntu)", &["wsl.exe", "wsl"])` 替换为发行版枚举，并在 `clink` 之后追加 VS 枚举：

```rust
        // WSL：枚举各发行版（每个作 `wsl -d <distro>`）；枚举不到则回退单条 bare wsl。
        if let Some(wsl) = lookup_program("wsl.exe").or_else(|| lookup_program("wsl")) {
            let prog = wsl.to_string_lossy().to_string();
            let distros = list_wsl_distros();
            if distros.is_empty() {
                found.push(ShellInfo {
                    id: "wsl".to_string(),
                    label: "WSL".to_string(),
                    program: prog,
                    args: vec![],
                });
            } else {
                for d in distros {
                    found.push(ShellInfo {
                        id: format!("wsl-{d}"),
                        label: format!("WSL: {d}"),
                        program: prog.clone(),
                        args: vec!["-d".to_string(), d],
                    });
                }
            }
        }
        try_add(&mut found, "nu", "Nushell", &["nu.exe", "nu"]);
        try_add(
            &mut found,
            "clink",
            "Clink (CMD 增强)",
            &["clink.exe", "clink", "cmder.exe", "Cmder.exe"],
        );
        found.extend(detect_vs_dev_shells());
```

（注意：删掉原 `try_add(&mut found, "wsl", ...)` 那一行，避免重复；`nu`/`clink` 两行保持，VS 枚举追加在最后。）

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p ridge-core parse_wsl_list`
Expected: PASS（3 个测试绿）

- [ ] **Step 6: Commit**

```bash
git add packages/ridge-core/src/commands/shell.rs
git commit -m "feat(shell): 枚举 WSL 各发行版 + VS 开发者环境（ShellInfo 加 args）"
```

> 已知限制（写入 commit body 或代码注释）：`args` 不持久化进 `.ridge`，恢复时 WSL/VS pane 会以 program 默认形态重启；全局默认 shell 设置仍只存 program。

---

### Task 4: per-pane 切换终端类型（③ + ④ 带参启动）

**Files:**
- Modify: `src-tauri/src/commands/terminal.rs`（`change_pane_shell` 持久化 `shell_kind` + 加 `args` 参数 + 带参走 `structured_command`）
- Modify: `src-tauri/src/commands/pane.rs`（`LayoutNode::Leaf` 加 `shell_kind`，`engine_node_to_layout` 填充）
- Modify: `src/lib/types.ts`（`PaneNode` leaf 加 `shell_kind?`）
- Modify: `src/lib/components/SplitContainer.svelte`（传 `currentShell` 给 `PaneShellSwitcher`）
- Modify: `src/lib/components/PaneShellSwitcher.svelte`（per-pane 标签 + 选择判定 + 传 args）

**Interfaces:**
- Consumes: `ShellInfo.args`（Task 3）；`ensure_pane_pty_workspace(.., shell: Option<String>, .., structured_command: Option<StructuredPtyCommand>, ..)`；`StructuredPtyCommand { program, args, env }`
- Produces: `change_pane_shell(paneId, shell, args)` 命令；layout leaf 带 `shell_kind`；`PaneShellSwitcher` prop `currentShell?: string`

- [ ] **Step 1: 后端 `change_pane_shell` 持久化 + 带参启动**

把 `terminal.rs::change_pane_shell`（`:63-94`）整体替换为：

```rust
#[tauri::command]
pub async fn change_pane_shell(
    state: State<'_, AppState>,
    pane_id: String,
    shell: String,
    args: Vec<String>,
) -> Result<(), String> {
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
    let workspace_id = state.active_workspace_id();
    let cwd = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.pane_tree.panes.get(&pane_id))
            .and_then(|p| p.cwd.clone())
    };

    teardown_pane_pty_if_present(&state, workspace_id, pane_id);
    state.clear_pty_scrollback(workspace_id, pane_id);

    // 持久化本 pane 的 shell（program）——对齐 create_pane_inner，使标题/恢复一致。
    {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&workspace_id) {
            if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
                pane.shell_kind = Some(shell.clone());
            }
        }
    }

    // 带参（WSL 发行版 / VS 开发者环境）走 structured_command；它使
    // has_explicit_launch=true，自动跳过 OSC7 注入（避免与 VS 的 -Command 冲突）。
    let (shell_opt, sc) = if args.is_empty() {
        (Some(shell), None)
    } else {
        (
            None,
            Some(StructuredPtyCommand {
                program: shell,
                args,
                env: std::collections::HashMap::new(),
            }),
        )
    };

    ensure_pane_pty_workspace(
        &*state,
        workspace_id,
        pane_id,
        shell_opt,
        cwd.as_deref(),
        None,
        sc,
        None,
        None,
        None,
    )
    .map_err(|e| e.to_string())
}
```

- [ ] **Step 2: 后端 layout leaf 暴露 `shell_kind`**

在 `pane.rs` 的 `LayoutNode::Leaf`（`:30-42`）加字段：

```rust
    Leaf {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// 本 pane 当前 shell 的 program（用于 per-pane 切换器标签）。
        #[serde(skip_serializing_if = "Option::is_none")]
        shell_kind: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_state: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
    },
```

在 `engine_node_to_layout` 的 `EnginePaneNode::Leaf` 分支（`:69-77`）填充：

```rust
            LayoutNode::Leaf {
                id: id.to_string(),
                title: titles.get(id).cloned(),
                cwd: panes
                    .get(id)
                    .and_then(|p| p.cwd.as_ref().map(|c| c.to_string_lossy().into_owned())),
                shell_kind: panes.get(id).and_then(|p| p.shell_kind.clone()),
                agent_state,
                agent_id: agent_by_pane.get(id).cloned(),
            }
```

- [ ] **Step 3: 等常驻 dev 重建通过**

观察常驻 `tauri dev` 终端输出，确认 Rust 重新编译无错误（不要并行另跑 cargo）。
Expected: 编译通过、应用热重载。

- [ ] **Step 4: 前端 `PaneNode` 加 `shell_kind`**

在 `src/lib/types.ts` 的 `PaneNode` leaf 分支加：

```ts
  | {
      type: 'leaf';
      id: string;
      title?: string;
      cwd?: string;
      /** 本 pane 当前 shell 的 program（后端 get_pane_layout 回传，用于切换器标签）。 */
      shell_kind?: string;
      agent_state?: AgentState;
      agent_id?: string;
    }
```

- [ ] **Step 5: SplitContainer 传 `currentShell`**

在 `SplitContainer.svelte`（`:659`）把：

```svelte
<PaneShellSwitcher paneId={node.id} />
```

改为：

```svelte
<PaneShellSwitcher paneId={node.id} currentShell={node.shell_kind} />
```

- [ ] **Step 6: `PaneShellSwitcher` 改 per-pane**

改 `PaneShellSwitcher.svelte`：

(a) `ShellInfo` 接口加 `args`：

```ts
  interface ShellInfo {
    id: string;
    label: string;
    program: string;
    args: string[];
  }
```

(b) Props 加 `currentShell`，并加本地乐观选择 id：

```ts
  interface Props {
    paneId: string;
    currentShell?: string;
  }
  let { paneId, currentShell }: Props = $props();

  // 切换成功后立即记下选中的 ShellInfo.id（乐观）；layout 回传 shell_kind(program)
  // 在 WSL 多发行版同 program 时不足以区分，故优先用 selectedId。
  let selectedId = $state<string | null>(null);
```

(c) `getCurrentLabel()` 改为按 pane 当前 shell：

```ts
  function getCurrentLabel(): string {
    if (selectedId) {
      const byId = shells.find((s) => s.id === selectedId);
      if (byId) return byId.label;
    }
    if (currentShell) {
      const byProg = shells.find((s) => s.program === currentShell);
      if (byProg) return byProg.label;
    }
    if (shells.length > 0) return shells[0].label;
    return tr('workspace.shellFallback');
  }

  // 菜单内"当前项"判定：优先 selectedId，否则匹配 program。
  function isCurrent(s: ShellInfo): boolean {
    if (selectedId) return s.id === selectedId;
    return !!currentShell && s.program === currentShell;
  }
```

(d) `selectShell` 改为不再与全局默认比较、传 args、更新 selectedId：

```ts
  async function selectShell(shell: ShellInfo) {
    if (!isTauri()) return;
    open = false;
    if (isCurrent(shell)) return;
    changing = true;
    try {
      const wsId = $activeWorkspaceId;
      if (!wsId) return;
      const manager = TerminalManager.instance();
      manager.clearScrollback(paneId);
      await invoke('change_pane_shell', { paneId, shell: shell.program, args: shell.args ?? [] });
      await invoke('activate_pane_pty', {
        workspaceId: wsId,
        paneId,
        rows: manager.rows(paneId),
        cols: manager.cols(paneId),
      });
      selectedId = shell.id;
      manager.forceFullRedraw(paneId);
    } catch (e) {
      console.warn('change_pane_shell failed', e);
    } finally {
      changing = false;
    }
  }
```

(e) 模板里把两处 `s.program === $settingsStore.defaultShell` 判定改用 `isCurrent(s)`：

```svelte
            class="w-full flex items-center gap-2 px-3 py-1.5 text-[12px] text-left transition-colors
              {isCurrent(s)
                ? 'bg-[var(--rg-accent)]/12 text-[var(--rg-accent)]'
                : 'text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]'}"
            onclick={() => void selectShell(s)}
          >
            <span class="flex-1 truncate">{s.label}</span>
            {#if isCurrent(s)}
              <span class="text-[9px] text-[var(--rg-accent)]/70 uppercase tracking-wider">{$t('workspace.shellCurrent')}</span>
            {/if}
```

（`settingsStore`/`setSetting` 若不再被本组件使用则一并从 import 移除——`pnpm check` 会提示未用。`get` 同理。）

- [ ] **Step 7: 类型检查**

Run: `pnpm check`
Expected: PASS

- [ ] **Step 8: 运行时目测**

开两个 pane（split），各自用头部 shell 切换器选不同 shell（含 WSL 某发行版）：确认①每个 pane 标签独立显示自己的 shell；②选 WSL 发行版后该 pane 原地重建并真正进入该发行版；③可把某 pane 切回与全局默认相同的 shell。

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/commands/terminal.rs src-tauri/src/commands/pane.rs src/lib/types.ts src/lib/components/SplitContainer.svelte src/lib/components/PaneShellSwitcher.svelte
git commit -m "fix(terminal): per-pane 切 shell 持久化 shell_kind + 标签按 pane + 带参启动"
```

---

### Task 5: 拖拽停靠纯解析助手 + 单测（⑤a）

**Files:**
- Create: `src/lib/terminal/paneDockResolve.ts`
- Create: `src/lib/terminal/paneDockResolve.test.ts`

**Interfaces:**
- Consumes: `DockRegion`（`$lib/stores/paneTree`）
- Produces:
  - `regionAtPoint(clientX: number, clientY: number, el: { getBoundingClientRect(): DOMRect }): DockRegion`
  - `resolveDockTarget(el: Element | null, sourcePaneId: string, clientX: number, clientY: number): { paneId: string; region: DockRegion } | null`
  - `passedDragThreshold(startX, startY, x, y, threshold?): boolean`

- [ ] **Step 1: 先写失败测试**

Create `src/lib/terminal/paneDockResolve.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { regionAtPoint, passedDragThreshold } from './paneDockResolve';

function rectEl(x: number, y: number, w: number, h: number) {
  return {
    getBoundingClientRect: () =>
      ({ left: x, top: y, width: w, height: h, right: x + w, bottom: y + h, x, y, toJSON() {} }) as DOMRect,
  };
}

describe('regionAtPoint', () => {
  const el = rectEl(0, 0, 100, 100); // m=0.18 → 边带 18px
  it('左带 → left', () => expect(regionAtPoint(5, 50, el)).toBe('left'));
  it('右带 → right', () => expect(regionAtPoint(95, 50, el)).toBe('right'));
  it('上带 → top', () => expect(regionAtPoint(50, 5, el)).toBe('top'));
  it('下带 → bottom', () => expect(regionAtPoint(50, 95, el)).toBe('bottom'));
  it('中心 → center', () => expect(regionAtPoint(50, 50, el)).toBe('center'));
});

describe('passedDragThreshold', () => {
  it('小位移不算拖拽', () => expect(passedDragThreshold(0, 0, 2, 2)).toBe(false));
  it('超阈值算拖拽', () => expect(passedDragThreshold(0, 0, 10, 0)).toBe(true));
});
```

- [ ] **Step 2: 运行确认失败**

Run: `pnpm exec vitest run src/lib/terminal/paneDockResolve.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现助手**

Create `src/lib/terminal/paneDockResolve.ts`:

```ts
import type { DockRegion } from '$lib/stores/paneTree';

/** 与旧 SplitContainer.regionAtPoint 同语义：边带 18% 命中四向，否则 center。 */
export function regionAtPoint(
  clientX: number,
  clientY: number,
  el: { getBoundingClientRect(): DOMRect }
): DockRegion {
  const r = el.getBoundingClientRect();
  const x = (clientX - r.left) / Math.max(r.width, 1);
  const y = (clientY - r.top) / Math.max(r.height, 1);
  const m = 0.18;
  if (x < m) return 'left';
  if (x > 1 - m) return 'right';
  if (y < m) return 'top';
  if (y > 1 - m) return 'bottom';
  return 'center';
}

/** 从指针下的元素上溯到带 data-pane-id 的 pane 容器，算出停靠目标；
 *  命中源 pane 自身或无 pane 时返回 null。 */
export function resolveDockTarget(
  el: Element | null,
  sourcePaneId: string,
  clientX: number,
  clientY: number
): { paneId: string; region: DockRegion } | null {
  const wrapper = el?.closest('[data-pane-id]') as HTMLElement | null;
  if (!wrapper) return null;
  const paneId = wrapper.getAttribute('data-pane-id');
  if (!paneId || paneId === sourcePaneId) return null;
  return { paneId, region: regionAtPoint(clientX, clientY, wrapper) };
}

/** 起手位移是否超过阈值（避免点击被误判为拖拽）。 */
export function passedDragThreshold(
  startX: number,
  startY: number,
  x: number,
  y: number,
  threshold = 4
): boolean {
  return Math.abs(x - startX) >= threshold || Math.abs(y - startY) >= threshold;
}
```

- [ ] **Step 4: 运行确认通过**

Run: `pnpm exec vitest run src/lib/terminal/paneDockResolve.test.ts`
Expected: PASS（7 个测试绿）

- [ ] **Step 5: Commit**

```bash
git add src/lib/terminal/paneDockResolve.ts src/lib/terminal/paneDockResolve.test.ts
git commit -m "feat(workspace): pane 停靠命中/阈值纯助手 + 单测"
```

---

### Task 6: pane 头部拖拽停靠改指针事件（⑤b）

**Files:**
- Modify: `src/lib/stores/paneTree.ts`（新增 `paneDockHover` / `dragHoverWorkspaceId` 两个 store）
- Create: `src/lib/actions/paneDockDrag.ts`（Svelte action）
- Modify: `src/lib/components/SplitContainer.svelte`（leaf 加 `data-pane-id`；头部改 `use:paneDockDrag`；停靠层 hint 改读 store；删旧 DnD 代码）
- Modify: `src/lib/components/WorkspaceTabs.svelte`（tab 加 `data-ws-tab-id`；删旧 `onTabDragOver/Leave`；hover ring 改读 store）

**Interfaces:**
- Consumes: `resolveDockTarget` / `passedDragThreshold`（Task 5）；`dockPane` / `switchWorkspace` / `paneDragSourceId` / `activePaneId` / `activeWorkspaceId`（`$lib/stores/paneTree`）
- Produces: `paneDockHover: Writable<{ paneId: string; region: DockRegion } | null>`；`dragHoverWorkspaceId: Writable<string | null>`；`paneDockDrag(node, { paneId })` action

- [ ] **Step 1: 新增两个 store**

在 `src/lib/stores/paneTree.ts` 中 `paneDragSourceId` 定义附近加：

```ts
/** 指针拖拽 pane 时，当前悬停的停靠目标（leaf 据此画方向高亮）。 */
export const paneDockHover = writable<{ paneId: string; region: DockRegion } | null>(null);
/** 指针拖拽 pane 时，当前悬停的工作区 tab（tab 据此画 ring，命中 HOVER_SWITCH_MS 后切换）。 */
export const dragHoverWorkspaceId = writable<string | null>(null);
```

（`DockRegion` 已在本文件定义于 `:113`；`writable` 已 import。）

- [ ] **Step 2: 实现 `paneDockDrag` action**

Create `src/lib/actions/paneDockDrag.ts`:

```ts
import { get } from 'svelte/store';
import {
  paneDragSourceId,
  paneDockHover,
  dragHoverWorkspaceId,
  dockPane,
  switchWorkspace,
  activePaneId,
  activeWorkspaceId,
} from '$lib/stores/paneTree';
import { resolveDockTarget, passedDragThreshold } from '$lib/terminal/paneDockResolve';

const HOVER_SWITCH_MS = 250;

interface Params {
  paneId: string;
}

/** pane 头部拖拽手柄：指针事件实现"拖拽→停靠 / 跨工作区切换"，绕开 WebView2
 *  对 HTML5 DnD 的屏蔽（drag_drop_enabled=true 保留 OS 文件拖放）。 */
export function paneDockDrag(node: HTMLElement, params: Params) {
  let paneId = params.paneId;
  let startX = 0;
  let startY = 0;
  let dragging = false;
  let pointerId: number | null = null;
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let hoverTabWsId: string | null = null;

  function clearHover() {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
    }
    hoverTabWsId = null;
    dragHoverWorkspaceId.set(null);
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    pointerId = e.pointerId;
    startX = e.clientX;
    startY = e.clientY;
    dragging = false;
    node.setPointerCapture(e.pointerId);
    node.addEventListener('pointermove', onPointerMove);
    node.addEventListener('pointerup', onPointerUp);
    node.addEventListener('pointercancel', onPointerCancel);
  }

  function onPointerMove(e: PointerEvent) {
    if (pointerId === null) return;
    if (!dragging) {
      if (!passedDragThreshold(startX, startY, e.clientX, e.clientY)) return;
      dragging = true;
      paneDragSourceId.set(paneId);
    }
    const el = document.elementFromPoint(e.clientX, e.clientY);
    // 优先：悬停在非活动工作区 tab → 计时切换
    const tab = (el as HTMLElement | null)?.closest('[data-ws-tab-id]') as HTMLElement | null;
    const tabWsId = tab?.getAttribute('data-ws-tab-id') ?? null;
    if (tabWsId && tabWsId !== get(activeWorkspaceId)) {
      paneDockHover.set(null);
      if (hoverTabWsId !== tabWsId) {
        clearHover();
        hoverTabWsId = tabWsId;
        dragHoverWorkspaceId.set(tabWsId);
        hoverTimer = setTimeout(() => {
          if (get(paneDragSourceId) === paneId && hoverTabWsId === tabWsId) {
            void switchWorkspace(tabWsId);
          }
          hoverTimer = null;
        }, HOVER_SWITCH_MS);
      }
      return;
    }
    clearHover();
    // 否则：算 pane 停靠目标
    paneDockHover.set(resolveDockTarget(el, paneId, e.clientX, e.clientY));
  }

  async function finish(commit: boolean) {
    node.removeEventListener('pointermove', onPointerMove);
    node.removeEventListener('pointerup', onPointerUp);
    node.removeEventListener('pointercancel', onPointerCancel);
    if (pointerId !== null && node.hasPointerCapture(pointerId)) {
      node.releasePointerCapture(pointerId);
    }
    pointerId = null;
    const target = get(paneDockHover);
    const wasDragging = dragging;
    dragging = false;
    clearHover();
    paneDragSourceId.set(null);
    paneDockHover.set(null);
    if (!wasDragging) {
      // 未越过阈值 → 视作点击 → 聚焦本 pane。
      activePaneId.set(paneId);
      return;
    }
    if (commit && target && target.paneId !== paneId) {
      try {
        await dockPane(paneId, target.paneId, target.region);
      } catch (err) {
        console.error('dockPane failed', err);
      }
    }
  }

  function onPointerUp() {
    void finish(true);
  }
  function onPointerCancel() {
    void finish(false);
  }

  node.addEventListener('pointerdown', onPointerDown);

  return {
    update(p: Params) {
      paneId = p.paneId;
    },
    destroy() {
      node.removeEventListener('pointerdown', onPointerDown);
      clearHover();
    },
  };
}
```

- [ ] **Step 3: SplitContainer 接入指针拖拽 + 删旧 DnD**

在 `SplitContainer.svelte`：

(a) import 区加：

```svelte
import { paneDockDrag } from '$lib/actions/paneDockDrag';
```

并在 `import { ... paneDragSourceId, dockPane, ... } from '$lib/stores/paneTree'` 里补 `paneDockHover`（去掉不再用的 `dockPane` 若改由 action 调用——`dockPane` 仅 action 用，组件内可移除其 import 与 `onDockDrop`）。

(b) 叶子容器（`:547` 的 `<div class="relative flex flex-col h-full ...">`）加 `data-pane-id`：

```svelte
        <div
          class="relative flex flex-col h-full min-h-0 min-w-0 overflow-hidden shadow-[0_8px_32px_rgba(0,0,0,0.35)]"
          data-pane-id={node.id}
        >
```

(c) 停靠覆盖层（`:550-578`）：hint region 改读 store，移除所有 HTML5 DnD 事件：

```svelte
          {#if $paneDragSourceId && $paneDragSourceId !== node.id}
            {@const hover = $paneDockHover && $paneDockHover.paneId === node.id ? $paneDockHover.region : null}
            <div
              class="absolute inset-0 z-30 rounded-lg bg-black/25 transition-shadow pointer-events-none {dockHintClass(hover)}"
              role="region"
              aria-label={$t('workspace.dockHereLabel')}
            ></div>
          {/if}
```

（`pointer-events-none`：覆盖层不再需要接事件——命中靠 `data-pane-id`（叶子容器，是覆盖层的祖先）；让覆盖层不挡 `elementFromPoint` 上溯没问题，但即便挡住，`closest('[data-pane-id]')` 仍能上溯到容器。设 `pointer-events-none` 更稳，确保命中到底层容器。）

(d) 头部拖拽手柄（`:582-598`）：去掉 `draggable`/`ondragstart`/`ondragend`，挂 `use:paneDockDrag`；保留 `onkeydown`（键盘聚焦），去掉 `onclick`（由 action 在非拖拽 pointerup 聚焦）：

```svelte
            <div
              class="flex-1 min-w-0 cursor-grab active:cursor-grabbing py-1 select-none"
              title={$t('workspace.paneDragTitle')}
              onkeydown={(e) => e.key === 'Enter' && activePaneId.set(node.id)}
              role="presentation"
              use:paneDockDrag={{ paneId: node.id }}
            >
```

(e) 删除现已无用的：`getDockRegion`（`:93`）、`regionAtPoint`（`:128`）、`dockHover` 本地 `$state`（`:91`）、`onDockDrop`（`:501`）。`dockHintClass` 保留（仍被覆盖层用）。

- [ ] **Step 4: WorkspaceTabs 接入**

在 `WorkspaceTabs.svelte`：

(a) import 把 `paneDragSourceId` 换/补为 `dragHoverWorkspaceId`：

```svelte
import { dragHoverWorkspaceId } from '$lib/stores/paneTree';
```

（若 `paneDragSourceId`/`get` 在删掉 `onTabDragOver/Leave` 后不再使用，一并移除其 import。）

(b) tab 元素（`:278` 的 `<div class="rg-no-drag relative shrink-0 ...">`）：加 `data-ws-tab-id`，把 ring 高亮条件从 `hoverTimerWsId === ws.id` 改为 `$dragHoverWorkspaceId === ws.id`，删 `ondragover`/`ondragleave`：

```svelte
      <div class="rg-no-drag relative shrink-0 flex items-center gap-1 rounded-lg px-3 py-1.5 text-[12px] font-medium transition-colors border cursor-grab active:cursor-grabbing select-none
          {ws.id === activeWorkspaceId
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)] border-[var(--rg-accent)]/35'
            : 'text-(--rg-fg-muted) border-transparent hover:bg-white/5 hover:text-(--rg-fg)'}
          {$dragHoverWorkspaceId === ws.id ? 'ring-2 ring-[var(--rg-accent)]/60' : ''}"
        data-ws-tab-id={ws.id}
        title={editingId === ws.id ? undefined : $t('workspace.tabSwitchTo', { name: getWorkspaceName(ws) })}
        onclick={() => { if (editingId !== ws.id) onSwitch(ws.id); }}
        onkeydown={(e) => handleSelectKeydown(e, ws)}
        oncontextmenu={(e) => handleContextMenu(e, ws)}
        role="button"
        tabindex="0"
        >
```

(c) 删除 `onTabDragOver`/`onTabDragLeave` 函数及其用到的 `HOVER_SWITCH_MS`/`hoverTimer`/`hoverTimerWsId`/`clearHoverTimer`（`:206-249`）——这部分逻辑已迁进 action。

- [ ] **Step 5: 类型检查**

Run: `pnpm check`
Expected: PASS（若报未用 import，按提示删干净）

- [ ] **Step 6: 运行时目测（关键）**

在常驻 dev（真 Tauri 窗口，不是浏览器）里：
1. 单工作区开 2+ pane，拖某 pane 头部到另一 pane 的左/右/上/下/中，确认出现方向高亮、松手后正确停靠（含拖到终端画布上方时 capture 不丢、命中正常）。
2. 拖 pane 头部悬停到另一个工作区 tab 上 ~250ms，确认 tab 出现 ring 且自动切到该工作区，可继续在目标工作区停靠（跨工作区迁移）。
3. 单击 pane 头部仍只聚焦、不误触发拖拽。
4. 从资源管理器/系统拖一个文件进终端，确认 OS 文件拖放仍插入路径（未被破坏）。

- [ ] **Step 7: Commit**

```bash
git add src/lib/stores/paneTree.ts src/lib/actions/paneDockDrag.ts src/lib/components/SplitContainer.svelte src/lib/components/WorkspaceTabs.svelte
git commit -m "fix(workspace): pane 头部拖拽停靠改指针事件（绕开 WebView2 屏蔽 HTML5 DnD）"
```

---

## Self-Review

**Spec coverage：**
- ① 开关 → Task 2（Toggle 组件 + 远程控制处；"智能体设置"面板不在分支，已注明套用同组件）。✓
- ② 间距 → Task 1。✓
- ③ per-pane shell → Task 4（后端持久化 shell_kind + 暴露 layout；前端 per-pane 标签/判定）。✓
- ④ 检测补全 → Task 3（WSL 发行版 + VS，ShellInfo 带 args）+ Task 4（带参启动打通）。✓
- ⑤ 拖拽停靠 → Task 5（纯助手 + 单测）+ Task 6（指针 action + 接入，保留 OS 文件拖放）。✓

**Placeholder scan：** 无 TBD/TODO/"类似上文"；每个代码步骤含完整代码。VS Developer PowerShell 的 `Enter-VsDevShell` 调用给了可用版本，运行时若需微调参数（`-Arch` 等）属正常 tuning，结构完整。

**Type consistency：**
- `ShellInfo` 三处一致：Rust `{ id, label, program, args: Vec<String> }`（Task 3）↔ 前端 `{ id, label, program, args: string[] }`（Task 4 Step 6a）。✓
- `change_pane_shell(paneId, shell, args)`：后端签名（Task 4 Step 1）↔ 前端 invoke 入参 `{ paneId, shell, args }`（Task 4 Step 6d）。✓
- `LayoutNode::Leaf.shell_kind`（Task 4 Step 2）↔ `PaneNode.shell_kind`（Step 4）↔ `currentShell` prop（Step 5/6）。✓
- `paneDockHover` / `dragHoverWorkspaceId`（Task 6 Step 1）在 action（Step 2）、SplitContainer（Step 3c）、WorkspaceTabs（Step 4b）引用一致。✓
- `resolveDockTarget`/`regionAtPoint`/`passedDragThreshold`（Task 5）签名与 action 调用（Task 6 Step 2）一致。✓

**已知限制（非缺口，刻意 v1 范围）：**
- WSL/VS 的 `args` 不持久化进 `.ridge`；恢复时以 program 默认形态重启。
- 全局默认 shell 设置（SettingsPanel `<select>`）仍只存 program；WSL 多发行版同 program 时作全局默认会退化为 bare wsl（per-pane 切换器是选具体发行版的正路）。
- 结构化启动（带 args）的 shell 不注入 OSC7 cwd 跟踪（与现有 structured_command 行为一致）。
