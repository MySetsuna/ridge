import { describe, expect, it } from 'vitest';
import { communeText, communeUiText } from './communeUiText';

describe('Commune task and approval translations', () => {
  it('keeps Chinese and English copy in parity and interpolation intact', () => {
    expect(Object.keys(communeUiText.en).sort()).toEqual(Object.keys(communeUiText.zh).sort());
    expect(communeText('en', 'dispatch', { count: 3 })).toBe('Confirm dispatch (3)');
    expect(communeText('zh', 'dispatch', { count: 3 })).toBe('确认派发（3）');
  });

  it('falls back to Chinese for locales without a dedicated translation', () => {
    expect(communeText('ja', 'taskUnknown')).toBe(communeUiText.zh.taskUnknown);
  });
});
