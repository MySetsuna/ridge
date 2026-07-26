// 句级输入缓冲状态机钉死（语音/高频改写场景）。
import { describe, expect, it } from 'vitest';
import { SentenceBuffer } from './sentenceBuffer';

describe('SentenceBuffer', () => {
  it('明文追加与取走', () => {
    const b = new SentenceBuffer();
    b.insert('send ');
    b.insert('the ');
    b.insert('male');
    expect(b.preview).toBe('send the male');
    expect(b.takeFlush()).toBe('send the male');
    expect(b.empty).toBe(true);
  });

  it('语音回改：replaceTrailing 换最后一个词（保尾随空白）', () => {
    const b = new SentenceBuffer();
    b.insert('send the male ');
    expect(b.replaceTrailing('mail')).toBe(true);
    expect(b.preview).toBe('send the mail ');
  });

  it('缓冲为空时 replaceTrailing 返回 false（调用方走已落笔差量路径）', () => {
    const b = new SentenceBuffer();
    expect(b.replaceTrailing('mail')).toBe(false);
  });

  it('commit 前缀合并：Ev 流式后整词 Everything 替换尾词而非叠加', () => {
    const b = new SentenceBuffer();
    b.insert('Ev');
    b.commit('Everything');
    expect(b.preview).toBe('Everything');
  });

  it('commit 无关词（中文候选）追加，不动前文', () => {
    const b = new SentenceBuffer();
    b.insert('echo ');
    b.commit('你好');
    expect(b.preview).toBe('echo 你好');
  });

  it('backspace 本地削字符，空缓冲返回 false（放行 PTY 退格）', () => {
    const b = new SentenceBuffer();
    b.insert('ab');
    expect(b.backspace()).toBe(true);
    expect(b.preview).toBe('a');
    expect(b.backspace()).toBe(true);
    expect(b.backspace()).toBe(false);
  });

  it('组合场景：流式+commit+回改+追加连缀', () => {
    const b = new SentenceBuffer();
    b.insert('Ev');
    b.commit('Everything ');
    b.insert('is ');
    b.insert('fone');
    expect(b.replaceTrailing('fine')).toBe(true);
    expect(b.takeFlush()).toBe('Everything is fine');
  });
});
