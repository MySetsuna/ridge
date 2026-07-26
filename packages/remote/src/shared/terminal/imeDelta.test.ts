// iter-60 G11 — imeCommitDelta 行为钉死（对话原例 SpacSpace 为首用例）。
import { describe, expect, it } from 'vitest';
import {
  imeCommitDelta,
  trailingWord,
  updatePendingWord,
  pendingWordBackspace,
} from './imeDelta';

const BS = '\x7f';

describe('imeCommitDelta', () => {
  it('用户原例：已发 Spac、补全 Space → 只发差量 e（不再 SpacSpace）', () => {
    expect(imeCommitDelta('Spac', 'Space')).toBe('e');
  });

  it('修正型补全：已发 Spac、补全 Spice → 退格删 ac 再发 ice', () => {
    expect(imeCommitDelta('Spac', 'Spice')).toBe(`${BS}${BS}ice`);
  });

  it('无已输段（纯候选提交，如中文）→ 整词原样', () => {
    expect(imeCommitDelta('', '空间')).toBe('空间');
  });

  it('已输段与 commit 无公共前缀（换词）→ 整词原样，不误退格', () => {
    expect(imeCommitDelta('Spac', 'hello')).toBe('hello');
  });

  it('跨词：仅对最后一个词去重（前文含空格不参与）', () => {
    expect(imeCommitDelta('echo Spac', 'Space')).toBe('e');
  });

  it('完全重复提交（word === commit）→ 空串（全部去重）', () => {
    expect(imeCommitDelta('Space', 'Space')).toBe('');
  });

  it('commit 为空 → 空串', () => {
    expect(imeCommitDelta('abc', '')).toBe('');
  });

  it('word 比 commit 长（补全成短词）→ 退格到公共前缀', () => {
    expect(imeCommitDelta('Spaces', 'Space')).toBe(BS);
  });
});

describe('trailingWord', () => {
  it('取最后一个空白后的段', () => {
    expect(trailingWord('git com')).toBe('com');
    expect(trailingWord('abc ')).toBe('');
    expect(trailingWord('abc')).toBe('abc');
    expect(trailingWord('')).toBe('');
  });
});

describe('pendingWord 精确追踪（二修：无时窗）', () => {
  it('用户实测案：Ev 已发、第三键进组合态、停顿后选 Everything → 只发 erything', () => {
    let w = '';
    w = updatePendingWord(w, 'E');
    w = updatePendingWord(w, 'v');
    // 「e」在组合态只进预编辑不发 PTY——词段仍是 Ev；停顿任意久后 commit：
    expect(imeCommitDelta(w, 'Everything')).toBe('erything');
  });

  it('updatePendingWord：追加并按空白断词', () => {
    expect(updatePendingWord('', 'Ev')).toBe('Ev');
    expect(updatePendingWord('Ev', 'e')).toBe('Eve');
    expect(updatePendingWord('Eve', ' ')).toBe('');
    expect(updatePendingWord('git', ' com')).toBe('com');
    expect(updatePendingWord('a', 'b\n')).toBe('');
  });

  it('pendingWordBackspace 削尾一字符', () => {
    expect(pendingWordBackspace('Eve')).toBe('Ev');
    expect(pendingWordBackspace('')).toBe('');
  });

  it('控制键清段后（调用方置空）commit 整词直发，不误退格', () => {
    expect(imeCommitDelta('', 'Evil')).toBe('Evil');
  });
});
