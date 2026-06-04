/** mobile 命名空间：移动端远控 UI（src/remote/*）。 */
export const zh: Record<string, string> = {
  // MainApp
  noActiveTerminal: '无活跃终端',
  creating: '创建中…',
  newTerminal: '新建终端',
  createTerminalFailRetry: '新建终端失败，请重试',
  createTerminalFail: '新建终端失败',

  // TopBar
  workspaceDefault: '工作区',
  closeWorkspace: '关闭工作区',
  noWorkspace: '无工作区',
  terminalDefault: '终端',
  closeTerminal: '关闭终端',
  noTerminal: '无终端',
  newTerminalBtn: '新建终端',

  // BottomTabBar
  filesTitle: '文件',
  searchTitle: '搜索',
  virtualKeyboard: '虚拟键盘',
  lockAndRefresh: '锁定渲染尺寸到本端并刷新',
  newWorkspace: '新建工作区',

  // AuthScreen
  authSubtitle: '输入桌面端 Ridge 应用中显示的 6 位动态验证码',
  codePlaceholder: '输入 6 位验证码',
  invalidCode: '验证码无效',
  networkError: '网络错误，请重试',
  connectFail: '连接失败，请重新输入验证码',
  verifying: '验证中...',
  verifyAndConnect: '验证并连接',
  connecting: '正在连接远程桌面...',

  // CertTrustGuide — toggle
  certToggleLabel: '浏览器提示「不安全」？安装证书消除警告',
  // CertTrustGuide — panel
  certIntro: '本机使用自动生成的本地证书加密远程连接。安装并信任一次后，浏览器不再提示「不安全」，并解锁完整功能（含 GPU 加速终端渲染）。',
  certInsecure: '当前为 HTTP 连接，证书暂不可用。请确认桌面端已启用 HTTPS 后通过 {code} 地址重新打开本页。',
  // CertTrustGuide — iOS steps
  certIosStep1: '点击下方「下载证书」，在弹窗中选择「允许」下载描述文件。',
  certIosStep2: '打开「设置 → 通用 → VPN 与设备管理」，安装「Ridge Remote Local CA」描述文件。',
  certIosStep3: '打开「设置 → 通用 → 关于本机 → 证书信任设置」，为「Ridge Remote Local CA」打开完全信任。',
  certIosStep4: '返回浏览器刷新本页，地址栏的「不安全」警告即消除。',
  // CertTrustGuide — Android steps
  certAndroidStep1: '点击下方「下载证书」保存 ridge-remote-ca.crt。',
  certAndroidStep2: '打开「设置 → 安全 → 加密与凭据 → 安装证书 → CA 证书」（部分机型在「从存储设备安装」）。',
  certAndroidStep3: '选择刚下载的 ridge-remote-ca.crt 完成安装。',
  certAndroidStep4: '返回浏览器刷新本页。',
  // CertTrustGuide — Desktop steps
  certDesktopStep1: 'Windows：下载 .crt → 双击 → 安装证书 → 选择「受信任的根证书颁发机构」→ 完成。',
  certDesktopStep2: 'macOS：下载 .pem → 用「钥匙串访问」导入到「系统」→ 双击证书并设为「始终信任」。',
  certDesktopStep3: 'Linux/其他：将 .pem 导入浏览器或系统的受信任根证书。',
  certDesktopStep4: '重启浏览器后刷新本页。',
  // CertTrustGuide — download buttons
  downloadCert: '下载证书',
  downloadCertCrt: '下载证书 (.crt)',

  // RemoteSidebar
  sidebarFilesTitle: '文件',
  sidebarSearchTitle: '搜索',
  sidebarClose: '关闭',

  // TerminalCanvas
  initializingTerminal: '初始化终端引擎…',
  copied: '✓ 已复制',
  copy: '复制',

  // SidebarFileTree
  parentDir: '上级目录',
  refresh: '刷新',
  loading: '加载中…',
  emptyDir: '空目录',
};

export const en: Record<string, string> = {
  // MainApp
  noActiveTerminal: 'No active terminal',
  creating: 'Creating…',
  newTerminal: 'New terminal',
  createTerminalFailRetry: 'Failed to create terminal, please retry',
  createTerminalFail: 'Failed to create terminal',

  // TopBar
  workspaceDefault: 'Workspace',
  closeWorkspace: 'Close workspace',
  noWorkspace: 'No workspaces',
  terminalDefault: 'Terminal',
  closeTerminal: 'Close terminal',
  noTerminal: 'No terminals',
  newTerminalBtn: 'New terminal',

  // BottomTabBar
  filesTitle: 'Files',
  searchTitle: 'Search',
  virtualKeyboard: 'Virtual keyboard',
  lockAndRefresh: 'Lock render size to this device and refresh',
  newWorkspace: 'New workspace',

  // AuthScreen
  authSubtitle: 'Enter the 6-digit one-time code shown in the Ridge desktop app',
  codePlaceholder: 'Enter 6-digit code',
  invalidCode: 'Invalid code',
  networkError: 'Network error, please retry',
  connectFail: 'Connection failed, please re-enter the code',
  verifying: 'Verifying...',
  verifyAndConnect: 'Verify & Connect',
  connecting: 'Connecting to remote desktop...',

  // CertTrustGuide — toggle
  certToggleLabel: 'Browser showing "Not secure"? Install the certificate to fix',
  // CertTrustGuide — panel
  certIntro: 'This device uses an auto-generated local certificate to encrypt the remote connection. Install and trust it once, and the browser will no longer warn "Not secure" — and unlocks full features (including GPU-accelerated terminal rendering).',
  certInsecure: 'Currently on HTTP — the certificate is unavailable. Make sure HTTPS is enabled on the desktop, then reopen this page via {code}.',
  // CertTrustGuide — iOS steps
  certIosStep1: 'Tap "Download certificate" below and choose "Allow" to download the profile.',
  certIosStep2: 'Open Settings → General → VPN & Device Management and install the "Ridge Remote Local CA" profile.',
  certIosStep3: 'Open Settings → General → About → Certificate Trust Settings and enable full trust for "Ridge Remote Local CA".',
  certIosStep4: 'Return to the browser and refresh — the "Not secure" warning will be gone.',
  // CertTrustGuide — Android steps
  certAndroidStep1: 'Tap "Download certificate" below to save ridge-remote-ca.crt.',
  certAndroidStep2: 'Open Settings → Security → Encryption & credentials → Install a certificate → CA certificate (some devices: "Install from storage").',
  certAndroidStep3: 'Select the downloaded ridge-remote-ca.crt to complete installation.',
  certAndroidStep4: 'Return to the browser and refresh.',
  // CertTrustGuide — Desktop steps
  certDesktopStep1: 'Windows: Download .crt → double-click → Install certificate → select "Trusted Root Certification Authorities" → Finish.',
  certDesktopStep2: 'macOS: Download .pem → import into Keychain Access under "System" → double-click the cert and set to "Always Trust".',
  certDesktopStep3: 'Linux / other: Import the .pem into your browser or system trusted root store.',
  certDesktopStep4: 'Restart the browser, then refresh this page.',
  // CertTrustGuide — download buttons
  downloadCert: 'Download certificate',
  downloadCertCrt: 'Download certificate (.crt)',

  // RemoteSidebar
  sidebarFilesTitle: 'Files',
  sidebarSearchTitle: 'Search',
  sidebarClose: 'Close',

  // TerminalCanvas
  initializingTerminal: 'Initializing terminal engine…',
  copied: '✓ Copied',
  copy: 'Copy',

  // SidebarFileTree
  parentDir: 'Parent directory',
  refresh: 'Refresh',
  loading: 'Loading…',
  emptyDir: 'Empty directory',
};
