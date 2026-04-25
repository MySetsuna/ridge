# Pane Git Pill — 真实数据 + 非 git cwd 隐藏 验证笔记（第 36 轮）

用户连续 3 轮反馈"git 按钮在非 git 仓库也显示 / 用的是模拟数据"。
本轮做端到端审计 + 写测试锁住契约。

## 数据流（确认无 mock）

```
PTY shell 输出 OSC7 / 1.5s 轮询 cwd
        │
        ▼ Tauri event `pane-cwd-changed` / setPaneCwd()
paneCwdStore[`${workspaceId}:${paneId}`] = "/cwd/path"
        │
        ▼ SplitContainer.svelte:60 $effect
trackPaneGitStatus(paneId, cwd)   // paneGitStatus.ts
        │
        ▼ 250ms debounce
resolveInfoForCwd(cwd)
        │
        ├─ invoke('find_git_repo_root', { path: cwd })
        │     │
        │     ▼ git.rs::find_git_repo_root
        │     loop { if cur.join(".git").exists() → Some(cur);
        │            if !cur.pop() → None; }
        │
        ├─ if None → store[paneId] = null → 两个 pill 都 hide ✓
        │
        └─ if Some(repoRoot) → invoke('get_scm_status') + invoke('git_diff_summary')
                                → 真实 git 数据 → store[paneId] = info
```

**0 处 mock / hardcoded git 数据**：grep `mock` / `placeholder` / 字面
branch 名只命中输入框 placeholder 文案，不命中数据。

## 隐藏契约

| 状态                             | PaneGitPill | PaneDiffPill |
|---------------------------------|-------------|--------------|
| `paneCwdStore` 还没有此 paneId   | 不渲染       | 不渲染        |
| cwd 是空串                       | 不渲染       | 不渲染        |
| `find_git_repo_root` 返回 null   | 不渲染       | 不渲染        |
| `get_scm_status` 返回无 branch   | 不渲染       | 不渲染        |
| 仓库内任何子目录                  | 渲染（正常）  | 渲染          |

模板 gate 都是 `{#if info && info.branch}` —— 严格双 falsy 检查。

## 用户验证 3 步

1. **打开一个肯定不在任何 git 仓库下的目录**：例如 Windows 下
   `C:\Users\<you>\Music`（或 macOS `/tmp/non-git-test`）。在 Wind
   开个新 pane 切到这个目录：`cd C:\Users\you\Music`。
2. **看 pane 标题栏**：右侧 `GitBranch` 图标 pill 和 `FileText` 数字
   pill 都应该 **不存在**。
3. **如果还能看到 pill**，说明 `find_git_repo_root` 在你的 cwd 上找到了
   `.git` 祖先——`cd` 进的目录其实在某个 git 仓库内（典型陷阱：
   你曾经在父目录 `git init` 过，留下了 `.git`）。运行
   `git rev-parse --show-toplevel` 在该 cwd 验证；如果返回路径，那
   pill 显示是**正确**的，不是 mock。

## 测试锁

`src/lib/stores/paneGitStatus.test.ts` 第 36 轮新增 3 个 vitest case：

1. `clears the store entry when cwd is null` —— 锁 `null cwd → 删除
   store entry`。
2. `returns null for cwd outside any git repo` —— 锁 backend 返回 null
   时 store 也是 null（pill 渲染条件 false）。
3. `debounces rapid cwd bounces — only the last cwd resolves` —— 锁
   250ms debounce 行为，避免 cd 链发起 N 次后端调用。

未来任何回退（删除 gate、引入 mock seed、退化 debounce）会立即 fail。
