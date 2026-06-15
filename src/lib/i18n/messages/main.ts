/** main 命名空间：主页面 chrome / 布局 / 插件面板。 */
export const zh: Record<string, string> = {
  // layout gate
  remoteGateSubtitle: '输入桌面端 Ridge 应用中显示的 6 位动态验证码',
  remoteGatePlaceholder: '输入 6 位验证码',
  remoteGateVerifying: '验证中...',
  remoteGateConnect: '验证并连接',
  remoteGateConnecting: '正在连接远程桌面...',
  remoteGateErrReconnect: '连接失败，请重新输入验证码',
  remoteGateErrInvalidCode: '验证码无效',
  remoteGateErrNetwork: '网络错误，请重试',
  // 自签名证书未受信任时，同源 fetch('/verify') 会被浏览器静默拦截（ERR_CERT_*），
  // 在 JS 里只表现为不可区分的 "Failed to fetch"。这条提示替代笼统的"网络错误"，
  // 指引用户安装/信任本机 CA 后重试。
  remoteGateErrCert: '无法连接到 Ridge 远控服务。很可能是本机的自签名证书尚未受信任——请点击下方按钮安装证书，或在浏览器中点开地址栏的证书警告并选择"继续访问"，然后重试。',
  remoteGateTrustCert: '安装本机证书 (CA)',
  remoteGateErrCloud: 'Cloud 连接失败',
  // 跨子域登录态未能在子域生效（cookie 缺失/失效），已回主域一次仍未解决——停止无限回跳。
  remoteGateErrTenantLoginStuck: '登录态未能在本子域生效，请回主域重新登录后再打开远控入口。',

  // §4 云端 TOTP 二次验证（控制端）
  totpGateSubtitle: '已建立加密连接。请输入桌面端 Ridge「官方公网加速」页面显示的 6 位动态验证码',
  totpGateVerify: '验证并控制',
  totpGateErrInvalid: '验证码无效，请重新输入',
  totpGateErrNetwork: '验证超时或失败，请重试',

  // sidebar nav tooltips
  navFiles: '文件',
  navSearch: '搜索 (Ctrl+Shift+F)',
  navRemote: '远程控制',
  navSettings: '设置（外观、字体、搜索、扩展）',
  navAccount: '账户',
  accountLogout: '退出登录',
  accountNoName: '已登录',

  // sidebar panel headers
  sidebarGitHeader: '源代码管理',
  sidebarExplorerHeader: '资源管理器',
  noWorkspaceSelected: '请先选择一个工作区',

  // sidebar resize handle
  sidebarResizeExpand: '拖动展开侧边栏',
  sidebarResizeAdjust: '拖动调整侧边栏宽度',

  // saved workspaces popup
  savedWorkspacesTitle: '已保存工作区',
  savedWorkspacesBtn: '已保存工作区',
  savedWorkspacesBrowse: '浏览…',
  savedWorkspacesBrowseTitle: '从任意 .ridge 文件打开（OS 文件选择器）',
  savedWorkspacesEmpty: '~/ridge-workspaces 下暂无 .ridge 文件',
  savedWorkspacesDelete: '删除工作区',

  // toolbar buttons
  newWorkspaceBtn: '新建根工作区（独立分屏树与终端）',
  editorHide: '收起文件编辑器',
  editorShow: '展开文件编辑器',
  splitHorizontal: '左右分屏（当前选中窗格）',
  splitVertical: '上下分屏（当前选中窗格）',

  // window controls
  winMinimize: '最小化',
  winRestore: '还原',
  winMaximize: '最大化',
  winClose: '关闭',

  // file dialog
  openRidgeDialogTitle: '打开 .ridge 工作区',

  // context menu labels
  ctxSplitH: '水平分割',
  ctxSplitV: '垂直分割',
  ctxClosePane: '关闭当前窗格',
  ctxCloseOthers: '关闭其他窗格',
  ctxFocusPane: '聚焦当前窗格',
  ctxCopyCwd: '复制 cwd 路径',
  ctxRevealCwd: '在文件管理器中显示 cwd',
  ctxReveal: '在文件管理器中显示',
  ctxCloseOnlyPane: '关闭窗格',
  ctxFiles: '文件浏览器',
  ctxSearch: '搜索',
  ctxGit: '源代码管理',
  ctxNewWorkspace: '新建工作区',
  ctxRenameWorkspace: '重命名工作区',
  ctxCloseWorkspace: '关闭当前工作区',
  ctxOpenScm: '打开源代码管理',

  // dialog titles and messages
  dlgOpenFailTitle: '打开失败',
  dlgCloseFailTitle: '关闭失败',
  dlgCloseOthersTitle: '关闭其它窗格',
  dlgCloseOthersMsg: '将关闭 {count} 个窗格，仅保留当前窗格。继续？',
  dlgCloseOthersOk: '关闭',
  dlgRenameTitle: '重命名工作区',
  dlgRenameMsg: '输入新的工作区名称：',
  dlgRenamePlaceholder: '工作区名称',
  dlgRenameFailTitle: '重命名失败',
  dlgCopyCwdTitle: '复制路径',
  dlgCopyCwdMsg: '该 pane 还未上报 cwd。',
  dlgGitOpFailed: '{label} 失败',
  dlgNoGitRepo: '当前工作区中没有任何 pane 处于 git 仓库内。',
  devIssueTooltip: '开发排障入口',
  dlgDevIssueMsg: '排障入口：切换工作区报错请先看运行 ridge / cargo tauri dev 的终端日志（搜索 [ridge][pty]）。Claude split 需在 Ridge 内建终端中运行，并确保 tmux shim 在 PATH 上。若出现 0xc0000142 这类进程级崩溃，需同时查看 Windows 事件查看器（应用程序日志）。',

  // WorkspaceSummaryPanel
  workspacePaneCountTitle: '当前工作区 pane 数',

  // GlobalStatusPanel
  globalStatusPaneCount: '{wsLabel} · {count} 个 pane',
  globalStatusDefaultWs: '工作区 {seq}',

  // GlobalStatusPanel · native-session discovery (conditional)
  nativeSessionsHeader: '后台会话',
  nativeSessionsSummon: '召唤进当前工作区',
  nativeSessionsOpen: '查看',
};

export const en: Record<string, string> = {
  // layout gate
  remoteGateSubtitle: 'Enter the 6-digit code shown in the Ridge app on your desktop',
  remoteGatePlaceholder: 'Enter 6-digit code',
  remoteGateVerifying: 'Verifying...',
  remoteGateConnect: 'Verify & Connect',
  remoteGateConnecting: 'Connecting to remote desktop...',
  remoteGateErrReconnect: 'Connection failed, please enter the code again',
  totpGateSubtitle: 'Encrypted connection established. Enter the 6-digit code shown on the "Public Accelerate" page in the Ridge desktop app',
  totpGateVerify: 'Verify & Control',
  totpGateErrInvalid: 'Invalid code, please try again',
  totpGateErrNetwork: 'Verification timed out or failed, please try again',
  remoteGateErrInvalidCode: 'Invalid code',
  remoteGateErrNetwork: 'Network error, please try again',
  // Self-signed cert not trusted → same-origin fetch('/verify') is blocked by the
  // browser (ERR_CERT_*), surfacing only as an opaque "Failed to fetch" in JS.
  remoteGateErrCert: 'Could not reach the Ridge remote service. This is most likely an untrusted self-signed certificate — install the local certificate with the button below, or open the certificate warning in your browser and choose "proceed", then retry.',
  remoteGateTrustCert: 'Install local certificate (CA)',
  remoteGateErrCloud: 'Cloud connection failed',
  // Cross-subdomain login state never took effect on the subdomain (missing/
  // stale cookie); already bounced to the main domain once — stop looping.
  remoteGateErrTenantLoginStuck: 'Your sign-in did not take effect on this subdomain. Please sign in again on the main site, then reopen the remote entry.',

  // sidebar nav tooltips
  navFiles: 'Files',
  navSearch: 'Search (Ctrl+Shift+F)',
  navRemote: 'Remote Control',
  navSettings: 'Settings (Appearance, Font, Search, Extensions)',
  navAccount: 'Account',
  accountLogout: 'Log out',
  accountNoName: 'Signed in',

  // sidebar panel headers
  sidebarGitHeader: 'Source Control',
  sidebarExplorerHeader: 'Explorer',
  noWorkspaceSelected: 'Please select a workspace first',

  // sidebar resize handle
  sidebarResizeExpand: 'Drag to expand sidebar',
  sidebarResizeAdjust: 'Drag to resize sidebar',

  // saved workspaces popup
  savedWorkspacesTitle: 'Saved Workspaces',
  savedWorkspacesBtn: 'Saved Workspaces',
  savedWorkspacesBrowse: 'Browse…',
  savedWorkspacesBrowseTitle: 'Open from any .ridge file (OS file picker)',
  savedWorkspacesEmpty: 'No .ridge files in ~/ridge-workspaces',
  savedWorkspacesDelete: 'Delete workspace',

  // toolbar buttons
  newWorkspaceBtn: 'New root workspace (independent split tree & terminals)',
  editorHide: 'Collapse file editor',
  editorShow: 'Expand file editor',
  splitHorizontal: 'Split left/right (active pane)',
  splitVertical: 'Split top/bottom (active pane)',

  // window controls
  winMinimize: 'Minimize',
  winRestore: 'Restore',
  winMaximize: 'Maximize',
  winClose: 'Close',

  // file dialog
  openRidgeDialogTitle: 'Open .ridge workspace',

  // context menu labels
  ctxSplitH: 'Split horizontal',
  ctxSplitV: 'Split vertical',
  ctxClosePane: 'Close current pane',
  ctxCloseOthers: 'Close other panes',
  ctxFocusPane: 'Focus current pane',
  ctxCopyCwd: 'Copy cwd path',
  ctxRevealCwd: 'Reveal cwd in file manager',
  ctxReveal: 'Reveal in file manager',
  ctxCloseOnlyPane: 'Close pane',
  ctxFiles: 'File Explorer',
  ctxSearch: 'Search',
  ctxGit: 'Source Control',
  ctxNewWorkspace: 'New workspace',
  ctxRenameWorkspace: 'Rename workspace',
  ctxCloseWorkspace: 'Close current workspace',
  ctxOpenScm: 'Open Source Control',

  // dialog titles and messages
  dlgOpenFailTitle: 'Open failed',
  dlgCloseFailTitle: 'Close failed',
  dlgCloseOthersTitle: 'Close other panes',
  dlgCloseOthersMsg: 'This will close {count} pane(s), keeping only the current one. Continue?',
  dlgCloseOthersOk: 'Close',
  dlgRenameTitle: 'Rename workspace',
  dlgRenameMsg: 'Enter new workspace name:',
  dlgRenamePlaceholder: 'Workspace name',
  dlgRenameFailTitle: 'Rename failed',
  dlgCopyCwdTitle: 'Copy path',
  dlgCopyCwdMsg: 'This pane has not reported a cwd yet.',
  dlgGitOpFailed: '{label} failed',
  dlgNoGitRepo: 'No pane in the current workspace is inside a git repository.',
  devIssueTooltip: 'Dev troubleshooting entry',
  dlgDevIssueMsg: 'Troubleshooting: check the terminal running ridge / cargo tauri dev for logs (search [ridge][pty]). Claude split must run inside the Ridge built-in terminal with tmux shim on PATH. For 0xc0000142-style process crashes, also check Windows Event Viewer (Application log).',

  // WorkspaceSummaryPanel
  workspacePaneCountTitle: 'Pane count for current workspace',

  // GlobalStatusPanel
  globalStatusPaneCount: '{wsLabel} · {count} pane(s)',
  globalStatusDefaultWs: 'Workspace {seq}',

  // GlobalStatusPanel · native-session discovery (conditional)
  nativeSessionsHeader: 'Background sessions',
  nativeSessionsSummon: 'Summon into current workspace',
  nativeSessionsOpen: 'View',
};
