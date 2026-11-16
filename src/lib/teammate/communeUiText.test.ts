import { describe, expect, it } from 'vitest';
import { communeRuntimeLabel, communeText, communeUiText } from './communeUiText';

describe('Commune task and approval translations', () => {
  it('keeps Chinese and English copy in parity and interpolation intact', () => {
    expect(Object.keys(communeUiText.en).sort()).toEqual(Object.keys(communeUiText.zh).sort());
    expect(communeText('en', 'dispatch', { count: 3 })).toBe('Confirm dispatch (3)');
    expect(communeText('zh', 'dispatch', { count: 3 })).toBe('确认派发（3）');
  });

  it('falls back to Chinese for locales without a dedicated translation', () => {
    expect(communeText('ja', 'taskUnknown')).toBe(communeUiText.zh.taskUnknown);
  });

  it('localizes known Runtime labels and preserves unrecognized metadata', () => {
    expect(communeRuntimeLabel('en', 'Agent 回放与接入后记录；接入前完整度未独立验证')).toBe(communeText('en', 'historyReplay'));
    expect(communeRuntimeLabel('zh', 'embedded_resource')).toBe('嵌入资源');
    expect(communeRuntimeLabel('en', 'Agent 请求工具权限')).toBe('Agent requests tool permission');
    expect(communeRuntimeLabel('en', 'custom provider metadata')).toBe('custom provider metadata');
    expect(communeRuntimeLabel('en', '__proto__')).toBe('__proto__');
  });
});
