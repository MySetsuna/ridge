// src/lib/stores/editorWindow.ts
//
// 「独立窗口」协调器（主窗口侧）。把整个文件编辑器弹出为一个真正独立的 OS 窗口
// （Tauri WebviewWindow，label='editor'），承载所有标签页。
//
// 所有权转移模型（避免双编辑器分叉）：
//   • 弹出：把当前打开文件（含未保存内容）写入共享 localStorage，创建窗口，清空主
//     窗口编辑器，并注册 open 拦截器——此后主窗口的新 open 转发给独立窗口。
//   • 关闭独立窗口：窗口侧把它的文件快照 emit 回主窗口，主窗口重新载入并注销拦截器。
//   任意时刻只有一个编辑器表面。
//
// 非 Tauri / web-remote：浏览器没有 OS 窗口概念，popOutEditor 回退到 floating 模式。
//
// 跨窗口通道用 Tauri event（emitTo/listen，web-remote 下走 shim 的 no-op，但该分支
// 永不触达）。窗口创建用 @tauri-apps/api/webviewWindow（仅在 isTauri() 分支调用）。

import { writable } from 'svelte/store';
import { isTauri } from '@tauri-apps/api/core';
import { emitTo, listen, type UnlistenFn } from '@tauri-apps/api/event';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { fileEditorStore, type OpenFile, type OpenRequest } from './fileEditor';

const WEB_REMOTE = import.meta.env.RIDGE_WEB_REMOTE === true;

/** 独立编辑器窗口的固定 label（单实例：再次弹出聚焦既有窗口）。 */
export const EDITOR_WIN_LABEL = 'editor';
/** 弹出时承载文件快照的共享 localStorage key（同源多窗口共享）。EditorWindow 启动时读取。 */
export const HANDOFF_KEY = 'ridge-editor-window-handoff';
/** 主 → 独立窗口：转发一次新的 open 请求。 */
export const EVT_OPEN = 'editor-window-open-file';
/** 独立 → 主窗口：窗口关闭，交还文件快照。 */
export const EVT_CLOSED = 'editor-window-closed';

/** 编辑器当前是否已弹出到独立窗口（主窗口据此可显示「已弹出」状态等）。 */
export const editorPoppedOut = writable(false);

export interface HandoffPayload {
  files: OpenFile[];
  active: string | null;
}

/** 把一次 open 请求转发给独立窗口，并把窗口提到前台。 */
function forwardOpen(req: OpenRequest): void {
  void emitTo(EDITOR_WIN_LABEL, EVT_OPEN, req);
  void WebviewWindow.getByLabel(EDITOR_WIN_LABEL).then((w) => w?.setFocus());
}

/** 主窗口已弹出时安装的 open 拦截器：转发并提前返回（不在主窗口本地打开）。 */
const interceptor = (req: OpenRequest): boolean => {
  forwardOpen(req);
  return true;
};

/** 把编辑器恢复到主窗口（窗口关闭交还 / 错误回滚共用）。 */
function restoreToMain(payload?: HandoffPayload): void {
  fileEditorStore.setOpenInterceptor(null);
  if (payload && payload.files.length > 0) {
    fileEditorStore.loadFiles(payload.files, payload.active);
  }
  editorPoppedOut.set(false);
}

/**
 * 把整个编辑器弹出为独立 OS 窗口。非 Tauri / web-remote 回退到 floating 模式。
 * 已弹出时聚焦既有窗口。仅应在主窗口调用（独立窗口内菜单项不显示）。
 *
 * @param attempt 重试次数（内部使用，外部调用不传）。Tauri 的 WebviewWindow label
 *   'editor' 在 destroy() 后可能被异步保留导致第二次创建失败，重试一次可恢复。
 */
export async function popOutEditor(attempt = 1): Promise<void> {
  // 浏览器 / 远程：没有 OS 窗口，退化为现有的悬浮模式（行为一致不报错）。
  if (!isTauri() || WEB_REMOTE) {
    fileEditorStore.setDisplayMode('floating');
    return;
  }

  // 已有独立窗口 → 聚焦即可，不重复创建。
  const existing = await WebviewWindow.getByLabel(EDITOR_WIN_LABEL);
  if (existing) {
    await existing.setFocus();
    return;
  }

  // 快照当前标签（含未保存内容）写入共享 localStorage，供新窗口启动时读取。
  const snap = fileEditorStore.snapshot();
  if (snap.files.length === 0) return; // 无可弹出的内容
  try {
    localStorage.setItem(HANDOFF_KEY, JSON.stringify(snap));
  } catch {
    /* localStorage 配额 / 隐私模式失败：仍尝试创建空窗口由用户重开文件 */
  }

  const win = new WebviewWindow(EDITOR_WIN_LABEL, {
    url: 'index.html?win=editor',
    title: 'Ridge — 编辑器',
    width: 960,
    height: 720,
    minWidth: 480,
    minHeight: 320,
    // 原生窗口装饰：完全独立的窗口应有系统标题栏 / 最小化 / 最大化 / 关闭。
    decorations: true,
  });

  win.once('tauri://created', () => {
    // 创建成功：注册转发拦截器、置标志、清空主窗口编辑器（内容已交接给新窗口）。
    fileEditorStore.setOpenInterceptor(interceptor);
    editorPoppedOut.set(true);
    fileEditorStore.clearForHandoff();
  });
  win.once('tauri://error', (e) => {
    console.warn('[editorWindow] 创建独立窗口失败', e?.payload);
    // 回滚：主窗口保持原状，不清空。
    editorPoppedOut.set(false);
    fileEditorStore.setOpenInterceptor(null);
    // Tauri 在 destroy() 后 label 未完全释放时，getByLabel 返回 null 但
    // new WebviewWindow 仍会失败。300ms 后重试一次。
    if (attempt < 2) {
      setTimeout(() => void popOutEditor(2), 300);
    }
  });
}

/**
 * 主窗口启动时调用一次：监听独立窗口关闭事件，交还文件快照并注销拦截器。
 * 返回 unlisten（应用生命周期内长驻，调用方可忽略）。
 */
export async function initEditorWindowHost(): Promise<UnlistenFn | null> {
  if (!isTauri() || WEB_REMOTE) return null;
  return listen<HandoffPayload>(EVT_CLOSED, (e) => {
    restoreToMain(e.payload);
  });
}
