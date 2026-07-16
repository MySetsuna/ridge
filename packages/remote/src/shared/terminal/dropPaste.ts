// 把"拖入终端"的文件路径格式化成可经 bracketed-paste 粘进 PTY 的文本。
// 裸路径、空格连接、无引号、无末尾空格——与 clipboard paste 管线一致，
// 裸路径对 TUI（Claude Code 等）的图片附件识别最可靠。
export function formatDroppedPathsForPaste(paths: string[]): string {
	return paths
		.map((p) => p.trim())
		.filter((p) => p.length > 0)
		.join(' ');
}
