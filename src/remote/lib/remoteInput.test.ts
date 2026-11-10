import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import init, { TerminalKernel } from '@ridge/term-wasm';
import { captureRemoteKey, editingKeyForInput, encodeRemoteKey, encodeRemoteInput, PendingRemoteInput, type RemoteKey } from './remoteInput';
import type { TerminalInputContext } from '@ridge/remote/shared/transport/paneRpcScheduler';

beforeAll(async () => {
  await init({ module_or_path: readFileSync(new URL('../../../packages/ridge-term/pkg/ridge_term_bg.wasm', import.meta.url)) });
});

const key = (name: string, modifiers: Partial<RemoteKey> = {}): RemoteKey => ({
  key: name, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...modifiers,
});
const text = new TextDecoder();
const context = (epoch = 'epoch-a', activation = 1): TerminalInputContext => [
  { workspaceId: 'workspace', paneId: 'pane' }, epoch, activation,
];

describe('Pending Remote input', () => {
  it('preserves a pre-attach prefix, context-less continuation and control key until binding', () => {
    const pending = new PendingRemoteInput();
    pending.enqueue(new TextEncoder().encode('W'), null);
    expect(pending.drain(null)).toEqual([]);
    pending.enqueue(new TextEncoder().encode('rite-Output marker'), null);
    pending.enqueue(key('Enter'), null);
    expect(pending.drain(null)).toEqual([]);
    const kernel = new TerminalKernel(24, 80, 20);
    try {
      const inputs = pending.drain(context());
      expect(inputs.map(input => text.decode(encodeRemoteInput(kernel, input))).join(''))
        .toBe('Write-Output marker\r');
      expect(pending.drain(context())).toEqual([]);
    } finally { kernel.free(); }
  });

  it('continues under a new activation of the same terminal lifetime', () => {
    const pending = new PendingRemoteInput();
    pending.observe(context());
    pending.enqueue(new TextEncoder().encode('same terminal'), null);
    expect(pending.drain(context('epoch-a', 2)).map(input => input instanceof Uint8Array ? text.decode(input) : 'intent'))
      .toEqual(['same terminal']);
  });

  it('rejects the entire old-epoch batch and its tail until explicit retry', () => {
    const pending = new PendingRemoteInput();
    pending.observe(context());
    pending.enqueue(new TextEncoder().encode('old'), null);
    expect(pending.drain(context('epoch-b'))).toEqual([]);
    expect(pending.failed).toBe(true);
    expect(pending.enqueue(new TextEncoder().encode('tail'), context('epoch-b'))).toBe(false);
    pending.reset();
    pending.enqueue(new TextEncoder().encode('fresh'), context('epoch-b'));
    expect(pending.drain(context('epoch-b')).map(input => input instanceof Uint8Array ? text.decode(input) : 'intent'))
      .toEqual(['fresh']);
  });

  it('drops an overflowing batch without releasing its executable prefix', () => {
    const pending = new PendingRemoteInput(4);
    pending.enqueue(new Uint8Array([1, 2, 3]), null);
    expect(pending.enqueue(new Uint8Array([4, 5]), null)).toBe(false);
    expect(pending.failed).toBe(true);
    expect(pending.length).toBe(0);
    expect(pending.enqueue(new Uint8Array([6]), null)).toBe(false);
    expect(pending.drain(context())).toEqual([]);
  });

  it('owns queued data and prevents late producers after cancellation', () => {
    const pending = new PendingRemoteInput();
    const data = new Uint8Array([65]);
    const event = { ...key('ArrowUp') };
    pending.enqueue(data, null);
    pending.enqueue(event, null);
    data[0] = 66;
    event.key = 'ArrowDown';
    expect(pending.drain(context())).toEqual([new Uint8Array([65]), key('ArrowUp')]);
    pending.enqueue(data, context());
    pending.cancel();
    expect(pending.enqueue(data, context())).toBe(false);
    expect(pending.drain(context())).toEqual([]);
  });

  it('encodes an early IME commit using the binding terminal paste mode', () => {
    const pending = new PendingRemoteInput();
    pending.enqueue({ paste: '中文' }, null);
    const kernel = new TerminalKernel(24, 80, 20);
    try {
      kernel.feed(new TextEncoder().encode('\x1b[?2004h'));
      expect(pending.drain(context()).map(input => text.decode(encodeRemoteInput(kernel, input))))
        .toEqual(['\x1b[200~中文\x1b[201~']);
    } finally { kernel.free(); }
  });
});

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
