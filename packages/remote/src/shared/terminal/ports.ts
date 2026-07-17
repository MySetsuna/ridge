// @ridge/remote — shared/terminal 端口接口（R0 内核化范式：Ctx + Reader 端口注入）。
//
// manager 迁入本包后不得再直接 import 主 app 的 store / util。它对主 app 的
// 运行时依赖（终端设置、pane cwd、链接点击路由）经这组端口从外部注入：
//   主 app 侧 src/lib/terminal/hostPorts.ts 实现 HostPorts（包装 settingsStore /
//   paneCwdStore / linkResolver），app 启动时 TerminalManager.setHostPorts() 注入。
// 手机端可注入部分实现或全缺省——所有成员可选，缺失时 manager 优雅降级。

/** manager 读取的终端相关设置子集。SSOT: src/lib/stores/settings.ts `Settings`。 */
export interface TerminalSettingsSnapshot {
  /** 新 pane attach 时按此设 kernel scrollback 容量（SettingsPanel 滑块 100..10000）。 */
  terminalScrollbackLines: number;
}

export interface SettingsPort {
  /** 读当前终端设置快照。缺省 / 未 hydrate 时由 manager 回退到 opts 默认。 */
  get(): TerminalSettingsSnapshot;
}

/** 链接解析所需的 pane cwd 查询（OSC 7 报告值）。手机端可给空实现。 */
export interface CwdPort {
  /** 指定 pane 的当前 cwd；无则 undefined。 */
  current(workspaceId: string, paneId: string): string | undefined;
  /** 所有 pane 的当前 cwd 集合，用于「是否落在任一 cwd 树内」判断。 */
  all(): string[];
}

/** manager 注入的主 app 能力集合。全部可选：SSR / 手机端 / 预启动期缺失即降级。 */
export interface HostPorts {
  settings?: SettingsPort;
  cwd?: CwdPort;
  /** 纯文本路径 / URL 点击路由（CWD 内文件→ridge 编辑器、外链→系统浏览器、
   *  外部路径/目录→资源管理器）。经此避免 manager 直接 import $lib/utils/linkResolver
   *  （及其 monaco 传递依赖）。手机端可不实现。 */
  openTextLink?(spanText: string, ctx: { cwd: string | undefined; knownCwds: string[] }): void;
}
