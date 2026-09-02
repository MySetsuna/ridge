// @ridge/remote — shared/terminal 端口接口（R0 内核化范式：Ctx + Reader 端口注入）。
//
// manager / themeBridge / ptyBridge / paneShell 迁入本包后不得再直接 import 主 app 的
// store / util。它们对主 app 的运行时依赖（终端设置、字号、壁纸、pane cwd、workspace、
// 链接路由、Tauri 后端）经这组端口从外部注入：
//   主 app 侧 src/lib/terminal/hostPorts.ts 实现 HostPorts（包装 settingsStore /
//   termFontSize / activeBgImage / paneCwdStore / activeWorkspaceId / linkResolver /
//   @tauri-apps），app 启动时经 TerminalManager.setHostPorts() 注入；模块内经
//   TerminalManager.hostPorts() 读回。手机端可注入部分实现或全缺省——所有成员可选，
//   缺失时各模块优雅降级。

/** manager / themeBridge 读取的终端相关设置子集。SSOT: src/lib/stores/settings.ts。 */
export interface TerminalSettingsSnapshot {
	/** Active theme id; used only to trigger a theme bridge push. */
	themeId?: string;
  /** 新 pane attach 时按此设 kernel scrollback 容量（SettingsPanel 滑块 100..10000）。 */
  terminalScrollbackLines: number;
  /** 终端字体族（themeBridge pushFont 用）。 */
  terminalFontFamily: string;
  /** 默认 shell 程序（ptyBridge 重建 PTY 时用）。 */
  defaultShell: string;
}

export interface SettingsPort {
  /** 读当前终端设置快照。缺省 / 未 hydrate 时由消费方回退到默认。 */
  get(): TerminalSettingsSnapshot;
  /** 订阅设置变更（themeBridge：主题 / 字体变即重推）。Svelte store 语义：
   *  订阅即同步触发一次。返回 unsubscribe。 */
  subscribe(cb: (s: TerminalSettingsSnapshot) => void): () => void;
}

/** 终端字号（独立于 settings 的 termFontSize store）。 */
export interface TermSettingsPort {
  fontSize(): number;
  subscribe(cb: (size: number) => void): () => void;
}

/** 壁纸/背景图信号（themeBridge：bg 图激活时终端背景透明化 + 变更重推）。 */
export interface ThemesPort {
  /** 当前激活背景图 URL；无则 null。 */
  activeBgImageUrl(): string | null;
  /** 订阅背景图变更。返回 unsubscribe。 */
  subscribe(cb: () => void): () => void;
}

/** 当前活动 workspace 查询（paneShell 切 shell 时定位 workspace）。 */
export interface WorkspacePort {
  activeId(): string | undefined;
}

/** 链接解析所需的 pane cwd 查询（OSC 7 报告值）。手机端可给空实现。 */
export interface CwdPort {
  /** 指定 pane 的当前 cwd；无则 undefined。 */
  current(workspaceId: string, paneId: string): string | undefined;
  /** Stable root for this pane's workspace. */
  workspaceRoot?(workspaceId: string, paneId: string): string | undefined;
  /** 所有 pane 的当前 cwd 集合，用于「是否落在任一 cwd 树内」判断。 */
  all(): string[];
}

/** manager 注入的主 app 能力集合。全部可选：SSR / 手机端 / 预启动期缺失即降级。 */
export interface HostPorts {
  settings?: SettingsPort;
  termSettings?: TermSettingsPort;
  themes?: ThemesPort;
  workspace?: WorkspacePort;
  cwd?: CwdPort;
  /** 纯文本路径 / URL 点击路由（CWD 内文件→ridge 编辑器、外链→系统浏览器、
   *  外部路径/目录→资源管理器）。经此避免 manager 直接 import $lib/utils/linkResolver
   *  （及其 monaco 传递依赖）。手机端可不实现。 */
  openTextLink?(
    request: TerminalLinkOpenRequest,
  ): Promise<TerminalLinkOpenResult> | TerminalLinkOpenResult;
  /** Non-opening proof used by modifier-hover. Only existing workspace files validate. */
  validateTextLink?(
    request: TerminalLinkOpenRequest,
  ): Promise<TerminalLinkValidationResult> | TerminalLinkValidationResult;
}

export interface TerminalPathOrigin {
  kind: 'local' | 'headless' | 'remote' | 'rdg' | 'shared';
  hostId?: string;
  workspaceId: string;
  paneId: string;
}

export interface TerminalLinkOpenRequest {
  type: 'url' | 'path';
  href?: string;
  path?: string;
  line?: number;
  col?: number;
  directoryHint?: boolean;
  cwd?: string;
  workspaceRoot?: string;
  origin: TerminalPathOrigin;
}

export interface TerminalLinkOpenResult {
  handled: boolean;
  reason?: string;
}

export interface TerminalLinkValidationResult {
  valid: boolean;
  reason?: string;
}
