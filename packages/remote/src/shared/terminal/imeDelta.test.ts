// iter-60 G11 — imeCommitDelta 行为钉死（对话原例 SpacSpace 为首用例）。
import { describe, expect, it } from 'vitest';
import { imeCommitDelta, trailingWord } from './imeDelta';

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
