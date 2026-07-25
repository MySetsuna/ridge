// iter-60 G11 — IME/自动补全提交去重（手机 Remote）。
//
// 场景：软键盘预测/自动补全下，用户逐字输入的字母已实时发往 PTY（如 `Spac`），
// 随后点选补全，输入法把**整词**（`Space`）作为一次 commit 再次交付 —— 若原样
// 转发，PTY 端变成 `SpacSpace`。
//
// 机制（用户点名的「标记已输入部分 + 补全时去重重复部分」）：取已发送缓冲的
// 尾部「当前词」（最后一个空白符之后的段），与 commit 求最长公共前缀：
//   - 公共前缀部分已经在 PTY 里，不再发送；
//   - 当前词多出的尾巴（用户已敲但被补全修正掉的字符）用退格 \x7f 逐个删除；
//   - 再发 commit 余下的差量。
// 例：typed=`Spac` + commit=`Space`  → 发 `e`
//     typed=`Spac` + commit=`Spice`  → 发 \x7f\x7f + `ice`（先删 `ac` 再补）
//     typed=``     + commit=`Space`  → 发 `Space`（无已输段，整词直发）
//
// 纯函数、无 DOM 依赖，便于 vitest 钉死行为。

/** ASCII DEL——终端语义里的「退一格删除」，与软键盘 Backspace 一致。 */
const BACKSPACE = '\x7f';

/** `recentTyped` 尾部的「当前词」：最后一个空白（空格/换行/Tab）之后的段。 */
export function trailingWord(recentTyped: string): string {
  const m = /[^\s]*$/.exec(recentTyped);
  return m ? m[0] : '';
}

/** 两串的最长公共前缀长度（按码元；补全场景均为 ASCII/BMP，够用且稳定）。 */
function commonPrefixLen(a: string, b: string): number {
  const n = Math.min(a.length, b.length);
  let i = 0;
  while (i < n && a[i] === b[i]) i += 1;
  return i;
}

/**
 * 计算补全 commit 相对已发送缓冲应实际发送的字节串。
 *
 * @param recentTyped 近窗口内已逐字发往 PTY 的文本（调用方负责时窗有效性）。
 * @param commit      输入法本次 commit 的完整文本。
 * @returns 应发送的串：`退格×N + 差量`。当无已输段可去重时 === commit 原文。
 */
export function imeCommitDelta(recentTyped: string, commit: string): string {
  if (!commit) return '';
  const word = trailingWord(recentTyped);
  if (!word) return commit;
  const lcp = commonPrefixLen(word, commit);
  if (lcp === 0) return commit; // 与已输段无关（如中文候选）——整词直发。
  return BACKSPACE.repeat(word.length - lcp) + commit.slice(lcp);
}
