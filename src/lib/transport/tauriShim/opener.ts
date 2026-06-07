// src/lib/transport/tauriShim/opener.ts
//
// Browser stand-in for `@tauri-apps/plugin-opener`. Aliased in by the web-remote
// Vite build. The desktop UI uses this plugin to hand a URL/path off to the OS
// default app; in a plain browser the closest faithful behavior is opening the
// target in a new tab. Surface mirrors the real package (`openUrl` / `openPath`
// / `revealItemInDir`) so any current or future import resolves cleanly.
//
// SPA 实际只用到 `openUrl`（外链 / 终端识别到的 URL），但补齐另两个导出可避免
// 远控构建下 import 该模块时缺符号；它们在浏览器里无主机文件系统等价行为，
// 故为安全降级（warn 后 no-op），不会抛错破坏页面。

/** `openWith` 在浏览器里无意义（无法指定 OS 应用），忽略即可。 */
type OpenWith = 'inAppBrowser' | string;

/**
 * 在浏览器新标签页打开外链。对应桌面 `openUrl`（OS 默认浏览器打开）。
 * `noopener` 切断 `window.opener` 引用，避免被打开页反向操纵本页（安全）。
 */
export async function openUrl(url: string | URL, _openWith?: OpenWith): Promise<void> {
  void _openWith;
  const href = typeof url === 'string' ? url : url.toString();
  if (typeof window !== 'undefined') {
    window.open(href, '_blank', 'noopener');
  }
}

/**
 * 桌面用 OS 默认程序打开主机本地路径。浏览器无主机文件系统访问能力，
 * 远控下 host 文件应走应用内编辑器（fileEditorStore）而非此插件，故这里
 * 仅记录降级、不尝试把裸路径塞进 `window.open`（会被当作相对 URL，无意义）。
 */
export async function openPath(path: string, _openWith?: string): Promise<void> {
  void _openWith;
  console.warn('[tauriShim/opener] openPath is not supported in web-remote mode', path);
}

/**
 * 桌面在系统资源管理器中定位文件。浏览器无等价能力，降级为 no-op。
 * 远控下"在资源管理器中显示"应改走 host 命令（见 linkResolver 的 reveal 分支
 * 经 `reveal_in_file_manager` 隧道），不依赖本插件。
 */
export async function revealItemInDir(path: string | string[]): Promise<void> {
  console.warn('[tauriShim/opener] revealItemInDir is not supported in web-remote mode', path);
}
