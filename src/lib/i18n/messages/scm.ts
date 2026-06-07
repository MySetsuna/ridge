/** scm 命名空间：源代码管理 / Git 面板 / pane git 标记。 */
export const zh: Record<string, string> = {
  // ─── Section headers ───────────────────────────────────────────────────────
  changesSection: '更改',
  graphSection: '图谱',

  // ─── Repo header ───────────────────────────────────────────────────────────
  repoCount: '{count} 仓库',
  refresh: '刷新',
  scanning: '扫描中…',
  noRepoDetected: '未在任意终端的 cwd 中检测到 Git 仓库。',
  expand: '展开',
  collapse: '折叠',
  currentBranch: '当前分支：{branch}（点击切换）',
  switchBranch: '切换分支',
  syncTooltip: '同步（fetch + pull + push）',
  newBranchPlaceholder: '新分支名称',
  createNewBranch: '创建新分支…',

  // ─── Group headers ─────────────────────────────────────────────────────────
  staged: '已暂存',
  unstageAll: '撤销暂存全部',
  changes: '更改',
  discardAllUnstaged: '丢弃全部未暂存更改',
  stageAll: '暂存全部',
  untracked: '未跟踪',
  deleteAllUntracked: '永久删除全部未跟踪文件',
  stageAllUntracked: '暂存全部未跟踪文件',

  // ─── Per-file actions ──────────────────────────────────────────────────────
  unstage: '撤销暂存',
  discardChange: '丢弃更改',
  stageChange: '暂存更改',
  deleteFilePermanent: '永久删除文件（不可撤销）',
  stage: '暂存',

  // ─── Commit box ────────────────────────────────────────────────────────────
  commitMessagePlaceholder: '消息（仅提交已暂存的更改）',
  commitButton: '提交 {count}',
  commitTooltip: '提交已暂存的更改',
  commitDisabledTooltip: '请先暂存文件',
  amendTooltip: '修改最近一次提交（git commit --amend）',
  workingTreeClean: '工作区干净',
  loading: '加载中…',

  // ─── Graph panel ───────────────────────────────────────────────────────────
  graphRefreshTooltip: '刷新（git pull --ff-only 后重载图谱；pull 失败仍刷新）',
  noRepoToShow: '无 Git 仓库可显示',
  rightClickForActions: '右键查看操作',
  loadingCommitFiles: '加载变动文件…',
  noChangedFiles: '无变动文件',
  viewFileDiffTooltip: '查看 {path} 在此 commit 的 diff',
  loadingOlder: '加载更早…',
  gitHistoryEnd: '已到 git 历史末端',

  // ─── Dialog titles / messages ──────────────────────────────────────────────
  copyFailed: '复制失败',
  copyFailedMsg: '复制{label}失败：{error}',
  cherryPickPaused: '仓库目前处于 cherry-pick 暂停状态。要 abort 并恢复工作树吗？',
  revertPaused: '仓库目前处于 revert 暂停状态。要 abort 并恢复工作树吗？',
  abortFailed: 'Abort 失败',
  stageFailed: '暂存失败',
  unstageFailed: '撤销暂存失败',
  discardFailed: '丢弃失败',
  commitFailed: '提交失败',
  switchBranchFailed: '切换分支失败',
  createBranchFailed: '创建分支失败',
  opFailed: '操作失败',
  commitMessageRequired: '请输入提交信息',
  commitMessageEmpty: '提交信息不能为空',
  noChangedFilesTitle: '无变动文件',
  noFilesInCommit: '{hash} 不包含可显示的文件改动。',

  // ─── Discard dialog ────────────────────────────────────────────────────────
  discardMixedMsg: '丢弃 {total} 个文件的更改？将永久删除 {untracked} 个未跟踪文件，此操作不可撤销。',
  deleteUntrackedOnlyMsg: '永久删除 {untracked} 个未跟踪文件？此操作不可撤销。',
  discardTrackedOnlyMsg: '丢弃 {total} 个文件的更改？此操作不可撤销。',
  deleteUntrackedTitle: '永久删除未跟踪文件',
  confirmDiscardTitle: '确认丢弃',
  deleteLabel: '删除',
  discardLabel: '丢弃',

  // ─── Dynamic op-failure label ──────────────────────────────────────────────
  opFailedLabel: '{label} 失败',
  revertCommitMsg: 'Revert {hash}？将创建一个反向 commit 撤销其改动。',
  pushTooltip: 'Push（无 upstream 时自动 -u origin HEAD）',

  // ─── Context-menu items ────────────────────────────────────────────────────
  copyShortHash: '复制短 hash ({hash})',
  copyFullHash: '复制完整 hash',
  createBranchFromCommit: '从此 commit 创建分支…',
  createBranchTitle: '创建分支',
  createBranchMsg: '从 {hash} 创建新分支并切过去：',
  checkoutDetachedMsg: 'Checkout 到 {hash}？这会进入 detached HEAD 状态——你现在不会在任何分支上。',
  createTagTitle: '创建 tag',
  createTagMsg: '在 {hash} 上创建标签：',
  annotatedTagTitle: 'Annotated tag 信息',
  annotatedTagMsg: '可选。留空则创建 lightweight tag（无 message）。',
  resetToCommit: 'Reset 到此 commit',
  resetSoftLabel: 'Soft  (保留索引与工作区改动)',
  resetMixedLabel: 'Mixed (保留工作区改动，清空索引)',
  resetHardLabel: 'Hard  (丢弃所有未提交改动 ‼)',
  resetHardMsg: 'Reset --hard 到 {hash}？\n\n会丢弃所有未提交的改动，且无法恢复。',

  // ─── PaneGitPill ───────────────────────────────────────────────────────────
  pillTooltip: '{repoRoot}\n分支：{branch}',
  pillTooltipAheadBehind: '\n↑{ahead} ↓{behind}',
  pillTooltipNoUpstream: '\n⚠ 当前分支没有 upstream — push 时会需要 -u origin <branch>',
  pillTooltipClickHint: '\n点击切换分支（Ctrl-Click 打开 SCM 侧栏）',
  noUpstreamAriaLabel: '无 upstream',
  switchedToBranch: '已切换到 {branch}',
  createdAndSwitched: '已创建并切换到 {branch}',
  createBranchCheckoutTooltip: '创建新分支并切过去（git checkout -b）',
  newBranchName: '新分支名',
  basedOn: '基于：',
  baseRefPlaceholder: 'HEAD（当前）',
  newBaseRefTitle: '新分支从此 ref 拉出（留空 = 当前 HEAD）',
  loadingBranches: '加载分支中…',
  noBranchInfo: '无分支信息',
  filterBranches: '过滤分支…',
  noMatchingBranch: '无匹配分支',
  openInSourceControl: '在源代码管理中打开',
  openInSourceControlTooltip: '打开 Source Control 侧栏，查看完整变更 / fetch / push',

  // ─── PaneDiffPill ──────────────────────────────────────────────────────────
  diffPillTooltip: '改动文件：{dirtyFiles}\n+{added} -{removed}\n点击在源代码管理中查看此仓库',

  // ─── SidebarGitPanel ───────────────────────────────────────────────────────
  notGitRepo: '非 Git 仓库',
  notGitRepoMsg: '当前目录不是 Git 仓库',
  changesCount: '变更 ({count})',
  recentCommits: '最近提交',
};

export const en: Record<string, string> = {
  // ─── Section headers ───────────────────────────────────────────────────────
  changesSection: 'Changes',
  graphSection: 'Graph',

  // ─── Repo header ───────────────────────────────────────────────────────────
  repoCount: '{count} repo(s)',
  refresh: 'Refresh',
  scanning: 'Scanning…',
  noRepoDetected: 'No Git repository detected in any terminal cwd.',
  expand: 'Expand',
  collapse: 'Collapse',
  currentBranch: 'Current branch: {branch} (click to switch)',
  switchBranch: 'Switch branch',
  syncTooltip: 'Sync (fetch + pull + push)',
  newBranchPlaceholder: 'New branch name',
  createNewBranch: 'Create new branch…',

  // ─── Group headers ─────────────────────────────────────────────────────────
  staged: 'Staged',
  unstageAll: 'Unstage all',
  changes: 'Changes',
  discardAllUnstaged: 'Discard all unstaged changes',
  stageAll: 'Stage all',
  untracked: 'Untracked',
  deleteAllUntracked: 'Permanently delete all untracked files',
  stageAllUntracked: 'Stage all untracked files',

  // ─── Per-file actions ──────────────────────────────────────────────────────
  unstage: 'Unstage',
  discardChange: 'Discard change',
  stageChange: 'Stage change',
  deleteFilePermanent: 'Permanently delete file (irreversible)',
  stage: 'Stage',

  // ─── Commit box ────────────────────────────────────────────────────────────
  commitMessagePlaceholder: 'Message (commit staged changes only)',
  commitButton: 'Commit {count}',
  commitTooltip: 'Commit staged changes',
  commitDisabledTooltip: 'Stage files first',
  amendTooltip: 'Amend the last commit (git commit --amend)',
  workingTreeClean: 'Working tree clean',
  loading: 'Loading…',

  // ─── Graph panel ───────────────────────────────────────────────────────────
  graphRefreshTooltip: 'Refresh (git pull --ff-only then reload graph; continues on pull failure)',
  noRepoToShow: 'No Git repository to display',
  rightClickForActions: 'Right-click for actions',
  loadingCommitFiles: 'Loading changed files…',
  noChangedFiles: 'No changed files',
  viewFileDiffTooltip: 'View diff of {path} in this commit',
  loadingOlder: 'Loading older…',
  gitHistoryEnd: 'Reached end of git history',

  // ─── Dialog titles / messages ──────────────────────────────────────────────
  copyFailed: 'Copy failed',
  copyFailedMsg: 'Failed to copy {label}: {error}',
  cherryPickPaused: 'Repository is paused mid cherry-pick. Abort and restore working tree?',
  revertPaused: 'Repository is paused mid revert. Abort and restore working tree?',
  abortFailed: 'Abort failed',
  stageFailed: 'Stage failed',
  unstageFailed: 'Unstage failed',
  discardFailed: 'Discard failed',
  commitFailed: 'Commit failed',
  switchBranchFailed: 'Switch branch failed',
  createBranchFailed: 'Create branch failed',
  opFailed: 'Operation failed',
  commitMessageRequired: 'Enter a commit message',
  commitMessageEmpty: 'Commit message cannot be empty',
  noChangedFilesTitle: 'No changed files',
  noFilesInCommit: '{hash} has no displayable file changes.',

  // ─── Discard dialog ────────────────────────────────────────────────────────
  discardMixedMsg: 'Discard changes to {total} files? {untracked} untracked file(s) will be permanently deleted. This cannot be undone.',
  deleteUntrackedOnlyMsg: 'Permanently delete {untracked} untracked file(s)? This cannot be undone.',
  discardTrackedOnlyMsg: 'Discard changes to {total} file(s)? This cannot be undone.',
  deleteUntrackedTitle: 'Permanently delete untracked files',
  confirmDiscardTitle: 'Confirm discard',
  deleteLabel: 'Delete',
  discardLabel: 'Discard',

  // ─── Dynamic op-failure label ──────────────────────────────────────────────
  opFailedLabel: '{label} failed',
  revertCommitMsg: 'Revert {hash}? This will create a reverse commit that undoes its changes.',
  pushTooltip: 'Push (auto -u origin HEAD when no upstream)',

  // ─── Context-menu items ────────────────────────────────────────────────────
  copyShortHash: 'Copy short hash ({hash})',
  copyFullHash: 'Copy full hash',
  createBranchFromCommit: 'Create branch from this commit…',
  createBranchTitle: 'Create branch',
  createBranchMsg: 'Create and checkout new branch from {hash}:',
  checkoutDetachedMsg: 'Checkout {hash}? This will enter detached HEAD state — you will not be on any branch.',
  createTagTitle: 'Create tag',
  createTagMsg: 'Create tag on {hash}:',
  annotatedTagTitle: 'Annotated tag message',
  annotatedTagMsg: 'Optional. Leave empty to create a lightweight tag (no message).',
  resetToCommit: 'Reset to this commit',
  resetSoftLabel: 'Soft  (keep index and working tree changes)',
  resetMixedLabel: 'Mixed (keep working tree changes, clear index)',
  resetHardLabel: 'Hard  (discard all uncommitted changes ‼)',
  resetHardMsg: 'Reset --hard to {hash}?\n\nAll uncommitted changes will be discarded and cannot be recovered.',

  // ─── PaneGitPill ───────────────────────────────────────────────────────────
  pillTooltip: '{repoRoot}\nBranch: {branch}',
  pillTooltipAheadBehind: '\n↑{ahead} ↓{behind}',
  pillTooltipNoUpstream: '\n⚠ Current branch has no upstream — push will require -u origin <branch>',
  pillTooltipClickHint: '\nClick to switch branch (Ctrl-Click opens SCM sidebar)',
  noUpstreamAriaLabel: 'No upstream',
  switchedToBranch: 'Switched to {branch}',
  createdAndSwitched: 'Created and switched to {branch}',
  createBranchCheckoutTooltip: 'Create new branch and check it out (git checkout -b)',
  newBranchName: 'New branch name',
  basedOn: 'Based on:',
  baseRefPlaceholder: 'HEAD (current)',
  newBaseRefTitle: 'Branch off this ref (empty = current HEAD)',
  loadingBranches: 'Loading branches…',
  noBranchInfo: 'No branch info',
  filterBranches: 'Filter branches…',
  noMatchingBranch: 'No matching branch',
  openInSourceControl: 'Open in Source Control',
  openInSourceControlTooltip: 'Open Source Control sidebar to view full changes / fetch / push',

  // ─── PaneDiffPill ──────────────────────────────────────────────────────────
  diffPillTooltip: 'Changed files: {dirtyFiles}\n+{added} -{removed}\nClick to inspect this repo in Source Control',

  // ─── SidebarGitPanel ───────────────────────────────────────────────────────
  notGitRepo: 'Not a Git repo',
  notGitRepoMsg: 'Current directory is not a Git repository',
  changesCount: 'Changes ({count})',
  recentCommits: 'Recent commits',
};
