// iter-60 追加 — 句级输入缓冲（用户拍板「语句缓冲要做」，针对语音/高频改写补全）。
//
// 动机：语音听写与激进预测键盘会**反复改写**刚产出的词/短语。直发 PTY 时只能靠
// 退格差量纠**最后一个词**（imeDelta），跨词回改只能丢弃。缓冲模式下，停顿
// `SENTENCE_FLUSH_MS`（或遇控制键/回车/粘贴）才把整段落笔 PTY——缓冲期内引擎
// 任意改写都发生在本地缓冲，**零退格、零丢失**。
//
// 视觉：缓冲文本由调用方画进预编辑覆盖层（与 IME preedit 同通道），肉眼零延迟；
// 终端回显延迟 = flush 间隔，故 alt-screen / TUI 鼠标态由调用方旁路本缓冲。
//
// 纯状态机、无计时器/DOM——计时与落笔时机归调用方（TerminalCanvas）。

/** 建议的停顿落笔间隔（ms）。语音词间隙通常 >800ms，600 兼顾打字回显。 */
export const SENTENCE_FLUSH_MS = 600;

function trailingWordParts(value: string): { start: number; word: string; whitespace: string } | null {
  let end = value.length;
  while (end > 0 && value[end - 1].trim() === '') end -= 1;
  if (end === 0) return null;
  let start = end;
  while (start > 0 && value[start - 1].trim() !== '') start -= 1;
  return { start, word: value.slice(start, end), whitespace: value.slice(end) };
}

export class SentenceBuffer {
  private buf = '';

  /** 当前缓冲全文（预编辑覆盖层显示用）。 */
  get preview(): string {
    return this.buf;
  }

  get empty(): boolean {
    return this.buf.length === 0;
  }

  /** 明文追加（handleInput 普通 insert）。 */
  insert(text: string): void {
    this.buf += text;
  }

  /**
   * IME/预测 commit（compositionend）：若缓冲尾词是 commit 的前缀（Ev + Everything
   * 之类「先流式后整词」），以 commit **替换**尾词；否则（中文候选等无关词）追加。
   */
  commit(text: string): void {
    if (!text) return;
    const parts = trailingWordParts(this.buf);
    if (parts && text.startsWith(parts.word)) {
      this.buf = this.buf.slice(0, parts.start) + text + parts.whitespace;
    } else {
      this.buf += text;
    }
  }

  /**
   * 引擎回改（insertReplacementText）：替换缓冲**最后一个词**（保尾随空白）。
   * 缓冲为空返回 false（调用方回退到已落笔差量路径）。
   */
  replaceTrailing(text: string): boolean {
    const parts = trailingWordParts(this.buf);
    if (!parts) return false;
    this.buf = this.buf.slice(0, parts.start) + text + parts.whitespace;
    return true;
  }

  /** Backspace：缓冲非空时本地削一字符并返回 true（不发 PTY 退格）。 */
  backspace(): boolean {
    if (!this.buf) return false;
    this.buf = this.buf.slice(0, -1);
    return true;
  }

  /** 取走待落笔全文并清空（调用方随后 onStdin 它，再发触发 flush 的控制字节）。 */
  takeFlush(): string {
    const out = this.buf;
    this.buf = '';
    return out;
  }
}
