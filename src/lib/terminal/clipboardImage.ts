// src/lib/terminal/clipboardImage.ts
//
// 把剪贴板里的图片落盘成临时 PNG（落在运行 PTY/CLI 的「服务器端」），返回可粘贴的绝对路径。
// 终端再把这个路径作为 bracketed-paste 文本粘进 TUI（Claude Code 等），由 CLI 识别为图片附件。
// 背景与后端落盘见 src-tauri/src/commands/clipboard_image.rs。
//
// 桌面 vs 远程 Web 的读图来源不同（用构建标志 RIDGE_WEB_REMOTE 分流，详见 tauriShim/core.ts）：
// - 桌面：后端直接读本机系统剪贴板（= 用户剪贴板）。
// - 远程：远程下 `invoke` 经 WS 路由到 host，后端 read_image 读到的是 host 剪贴板而非远程
//   用户的，故必须前端用浏览器剪贴板 API 读客户端图片，再经 save 命令落到 host 端（与 CLI 同端）。

import { invoke } from '@tauri-apps/api/core';

const WEB_REMOTE = import.meta.env.RIDGE_WEB_REMOTE === true;

/** Uint8Array → base64（分块，避免 String.fromCharCode 对大数组爆栈）。 */
function bytesToBase64(bytes: Uint8Array): string {
	let binary = '';
	const chunk = 0x8000;
	for (let i = 0; i < bytes.length; i += chunk) {
		binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
	}
	return btoa(binary);
}

/** 经后端把 PNG 字节落盘到服务器端临时目录，返回绝对路径。桌面/远程都走这里
 *  （远程下 invoke 经 WS 路由到 host 落盘，与 CLI 同端）。 */
async function savePngToTemp(png: Uint8Array): Promise<string> {
	return invoke<string>('save_clipboard_image_to_temp', { pngBase64: bytesToBase64(png) });
}

/** 用浏览器异步剪贴板 API 读第一张图片（image/*）转成字节。无图 / 不支持 / 无权限时返回 null。 */
async function readImageBytesFromBrowserClipboard(): Promise<Uint8Array | null> {
	// navigator.clipboard.read 仅 secure context 可用；Firefox 不支持（由 paste 事件兜底）。
	const clip = navigator.clipboard as Clipboard & { read?: () => Promise<ClipboardItem[]> };
	if (typeof clip?.read !== 'function') return null;
	let items: ClipboardItem[];
	try {
		items = await clip.read();
	} catch {
		return null;
	}
	for (const item of items) {
		const type = item.types.find((t) => t.startsWith('image/'));
		if (!type) continue;
		const blob = await item.getType(type);
		return new Uint8Array(await blob.arrayBuffer());
	}
	return null;
}

/** 主动从剪贴板取图片并落盘，返回路径；无图返回 null。
 *  桌面：后端读本机系统剪贴板；远程：前端读客户端剪贴板再经 host 落盘。 */
export async function acquireClipboardImagePath(): Promise<string | null> {
	if (WEB_REMOTE) {
		const png = await readImageBytesFromBrowserClipboard();
		if (!png) return null;
		return savePngToTemp(png);
	}
	return invoke<string | null>('read_clipboard_image_to_temp');
}

/** 从浏览器原生 paste 事件里取图片并落盘，返回路径；事件里没有图片返回 null。
 *  这条路桌面 / 远程都有效（paste 事件两端都带 clipboardData），尤其覆盖 Firefox。
 *  必须在第一个 await 前同步取出 File —— clipboardData 仅在事件派发期间有效。 */
export async function imagePathFromClipboardEvent(e: ClipboardEvent): Promise<string | null> {
	const items = e.clipboardData?.items;
	if (!items) return null;
	let file: File | null = null;
	for (let i = 0; i < items.length; i++) {
		const it = items[i];
		if (it.kind === 'file' && it.type.startsWith('image/')) {
			file = it.getAsFile();
			break;
		}
	}
	if (!file) return null;
	const png = new Uint8Array(await file.arrayBuffer());
	return savePngToTemp(png);
}
