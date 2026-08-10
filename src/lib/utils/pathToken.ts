// src/lib/utils/pathToken.ts
//
// 从一行文本 + 列号提取光标下的「路径样 token」——给 FileEditor 的 Ctrl+Click 路径
// 跳转用。纯字符串、不查 fs；真正的路径解析（相对/绝对/是否在工程内）交给
// linkResolver。末尾 `:line(:col)?` 解析为行列，从 path 剥离（支持 `foo.ts:42:7`、
// 编译器/stack-trace 风格的 `src/x.ts:10`）。URL（含端口）不被误解析为行列。

export interface PathToken {
  /** 去掉 `:line:col` 后缀后的路径/URL 片段。 */
  path: string;
  line?: number;
  col?: number;
}

// 路径允许字符：字母数字 + / \ . _ - ~ @ : （`:` 兼顾 Windows 盘符与 `:line` 后缀）。
// 引号 / 括号 / 空格被排除 —— 它们天然界定 import 说明符与字符串字面量的边界。
const PATH_CHAR = /[A-Za-z0-9_./\\~@:-]/;
const URL_SCHEME = /^[a-zA-Z][\w+.-]*:\/\//;

function parseLocationSuffix(raw: string): PathToken {
	if (URL_SCHEME.test(raw)) return { path: raw };
	let path = raw;
	let line: number | undefined;
	let col: number | undefined;
	const match = /:(\d+)(?::(\d+))?$/.exec(path);
	if (match?.index !== undefined) {
		line = Number(match[1]);
		col = match[2] ? Number(match[2]) : undefined;
		path = path.slice(0, match.index);
	}
	let end = path.length;
	while (end > 0 && '.,;'.includes(path[end - 1])) end -= 1;
	return { path: path.slice(0, end), line, col };
}

/**
 * 提取 Monaco `column`（1-based）处的路径 token。无可识别 token → null。
 * `column` 允许等于行长 + 1（光标在行尾）。点击恰在 token 右沿外时向左退一格。
 */
export function pathTokenAt(lineContent: string, column: number): PathToken | null {
  if (!lineContent) return null;
  const len = lineContent.length;
  const at = (k: number): string => (k >= 0 && k < len ? lineContent[k] : '');

  let i = Math.max(0, Math.min(column - 1, len)); // 0-based 光标索引（可 == len）
  if (!PATH_CHAR.test(at(i))) {
    if (PATH_CHAR.test(at(i - 1))) i -= 1;
    else return null;
  }

  let start = i;
  let end = i;
  while (start > 0 && PATH_CHAR.test(at(start - 1))) start -= 1;
  while (end < len - 1 && PATH_CHAR.test(at(end + 1))) end += 1;

	const parsed = parseLocationSuffix(lineContent.slice(start, end + 1));
	if (!parsed.path || parsed.path === '.' || parsed.path === '..') return null;
	return parsed;
}
