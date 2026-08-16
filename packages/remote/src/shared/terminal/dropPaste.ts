// 把“拖入终端”的文件路径格式化成可经 bracketed-paste 粘进 PTY 的文本。
// 路径含空白/引号时必须 shell-quote；否则 Windows 下 PowerShell/cmd 会把一个
// 文件拆成多个参数，Grok Build/图片附件等 TUI 便无法识别拖入项。
function quoteDroppedPath(path: string): string {
	if (!/[\s"'`]/.test(path)) return path;
	// Windows filenames cannot contain `"`; escaping keeps this safe for the
	// PowerShell/cmd path used by the desktop app and harmless in POSIX shells.
	return `"${path.replaceAll('"', '\\"')}"`;
}

export function formatDroppedPathsForPaste(paths: string[]): string {
	return paths
		.map((p) => p.trim())
		.filter((p) => p.length > 0)
		.map(quoteDroppedPath)
		.join(' ');
}
