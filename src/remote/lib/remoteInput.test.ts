import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import init, { TerminalKernel } from '@ridge/term-wasm';
import { captureRemoteKey, editingKeyForInput, encodeRemoteKey, type RemoteKey } from './remoteInput';

beforeAll(async () => {
  await init({ module_or_path: readFileSync(new URL('../../../packages/ridge-term/pkg/ridge_term_bg.wasm', import.meta.url)) });
});

const key = (name: string, modifiers: Partial<RemoteKey> = {}): RemoteKey => ({
  key: name, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...modifiers,
});
const text = new TextDecoder();

describe('Remote input with the real ridge-term encoder', () => {
  it('encodes navigation queued before attach using the installed terminal modes', () => {
    const event = { ...key('ArrowUp') };
    const queued = captureRemoteKey(event)!;
    event.key = 'ArrowDown';
    const kernel = new TerminalKernel(24, 80, 20);
    try {
      kernel.feed(new TextEncoder().encode('\x1b[?1h'));
      expect(text.decode(encodeRemoteKey(kernel, queued))).toBe('\x1bOA');
      kernel.feed(new TextEncoder().encode('\x1b[?1l'));
      expect(text.decode(encodeRemoteKey(kernel, queued))).toBe('\x1b[A');
    } finally { kernel.free(); }
  });

  it('preserves special-key modifiers rather than replacing their negotiated bytes', () => {
    const kernel = new TerminalKernel(24, 80, 20);
    try {
      expect(text.decode(encodeRemoteKey(kernel, key('Enter', { ctrlKey: true })))).toBe('\n');
      expect(text.decode(encodeRemoteKey(kernel, key('Tab', { shiftKey: true })))).toBe('\x1b[Z');
      expect(text.decode(encodeRemoteKey(kernel, key('Backspace')))).toBe('\x7f');
      expect(text.decode(encodeRemoteKey(kernel, key('ArrowRight', { ctrlKey: true })))).toBe('\x1b[1;5C');
    } finally { kernel.free(); }
  });

  it('leaves text/composition to input and maps mobile deletion/newline to key intent', () => {
    expect(captureRemoteKey(key('a'))).toBeNull();
    expect(captureRemoteKey(key('Process'))).toBeNull();
    expect(editingKeyForInput('deleteContentBackward')).toBe('Backspace');
    expect(editingKeyForInput('deleteContentForward')).toBe('Delete');
    expect(editingKeyForInput('insertParagraph')).toBe('Enter');
    expect(editingKeyForInput('insertCompositionText')).toBeNull();
  });
});
