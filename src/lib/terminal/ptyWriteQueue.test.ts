import { describe, expect, it, vi } from 'vitest';
import {
  enqueuePtyInput,
  enqueuePtyWrite,
  PtyWriteQueueFullError,
  PtyWriteQueueRetiredError,
  retirePtyWriteQueue,
} from './ptyWriteQueue';

describe('enqueuePtyWrite', () => {
	it('coalesces an optional short burst before the first IPC write', async () => {
		vi.useFakeTimers();
		try {
			const sent: string[] = [];
			const write = async (data: string) => { sent.push(data); };
			expect(enqueuePtyInput('ws:burst', '\x03', write, { coalesceWindowMs: 8 })).toBe(true);
			expect(enqueuePtyInput('ws:burst', '\x03', write, { coalesceWindowMs: 8 })).toBe(true);
			expect(sent).toEqual([]);
			await vi.advanceTimersByTimeAsync(8);
			expect(sent).toEqual(['\x03\x03']);
			retirePtyWriteQueue('ws:burst');
		} finally {
			vi.useRealTimers();
		}
	});

	it('coalesces keys queued behind a slow desktop write', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const sent: string[] = [];
    const write = async (data: string) => {
      sent.push(data);
      if (data === 'a') await gate;
    };

    expect(enqueuePtyInput('ws:input', 'a', write)).toBe(true);
    expect(enqueuePtyInput('ws:input', 'b', write)).toBe(true);
    expect(enqueuePtyInput('ws:input', 'c', write)).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(sent).toEqual(['a']);
    release();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(sent).toEqual(['a', 'bc']);
    retirePtyWriteQueue('ws:input');
  });

  it('bounds coalesced input bytes', () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    expect(enqueuePtyInput('ws:input-cap', 'a', async () => gate, { maxQueuedBytes: 2 })).toBe(true);
    expect(enqueuePtyInput('ws:input-cap', 'b', async () => undefined, { maxQueuedBytes: 2 })).toBe(true);
    expect(enqueuePtyInput('ws:input-cap', 'c', async () => undefined, { maxQueuedBytes: 2 })).toBe(false);
    release();
    retirePtyWriteQueue('ws:input-cap');
  });

  it('keeps multiline paste before later input for the same pane', async () => {
    const sent: string[] = [];
    let releasePaste!: () => void;
    const pasteGate = new Promise<void>((resolve) => { releasePaste = resolve; });
    const paste = enqueuePtyWrite('ws:pane', async () => {
      await pasteGate;
      sent.push('one\ntwo\nthree');
    });
    const key = enqueuePtyWrite('ws:pane', async () => { sent.push('x'); });

    await Promise.resolve();
    await Promise.resolve();
    expect(sent).toEqual([]);
    releasePaste();
    await Promise.all([paste, key]);
    expect(sent).toEqual(['one\ntwo\nthree', 'x']);
  });

  it('does not strand later input after a write failure', async () => {
    const sent: string[] = [];
    await expect(enqueuePtyWrite('ws:failure', async () => { throw new Error('closed'); })).rejects.toThrow('closed');
    await enqueuePtyWrite('ws:failure', async () => { sent.push('retry'); });
    expect(sent).toEqual(['retry']);
  });

  it('bounds pending operations before a slow write can grow memory', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const first = enqueuePtyWrite('ws:bounded', () => gate, { maxPending: 2 });
    const second = enqueuePtyWrite('ws:bounded', async () => undefined, { maxPending: 2 });

    await expect(
      enqueuePtyWrite('ws:bounded', async () => undefined, { maxPending: 2 }),
    ).rejects.toBeInstanceOf(PtyWriteQueueFullError);
    release();
    await Promise.all([first, second]);
  });

  it('retires queued writes when the pane closes', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const sent: string[] = [];
    const first = enqueuePtyWrite('ws:retire', async () => {
      await gate;
      sent.push('first');
    });
    const queued = enqueuePtyWrite('ws:retire', async () => {
      sent.push('stale');
    });

    await Promise.resolve();
    await Promise.resolve();
    retirePtyWriteQueue('ws:retire');
    release();
    await first;
    await expect(queued).rejects.toBeInstanceOf(PtyWriteQueueRetiredError);
    expect(sent).toEqual(['first']);
  });
});
