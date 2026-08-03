import { describe, expect, it } from 'vitest';
import {
  enqueuePaneInput,
  tryEnqueuePaneInput,
  retirePaneInput,
  PaneInputGateRetiredError,
} from './paneInputGate';

describe('paneInputGate', () => {
  it('reserves an async paste intent before later input', async () => {
    const sent: string[] = [];
    let release!: () => void;
    const wait = new Promise<void>((resolve) => { release = resolve; });
    const paste = enqueuePaneInput('gate:order', async () => {
      await wait;
      sent.push('paste');
    });
    const key = enqueuePaneInput('gate:order', () => { sent.push('key'); });

    await Promise.resolve();
    expect(sent).toEqual([]);
    release();
    await Promise.all([paste, key]);
    expect(sent).toEqual(['paste', 'key']);
  });

  it('continues after a failed intent', async () => {
    await expect(enqueuePaneInput('gate:failure', async () => {
      throw new Error('clipboard denied');
    })).rejects.toThrow('clipboard denied');
    const sent: string[] = [];
    await enqueuePaneInput('gate:failure', () => { sent.push('retry'); });
    expect(sent).toEqual(['retry']);
  });

  it('retires queued intents when a pane closes', async () => {
    let release!: () => void;
    const wait = new Promise<void>((resolve) => { release = resolve; });
    const first = enqueuePaneInput('gate:retire', async () => { await wait; });
    const queued = enqueuePaneInput('gate:retire', () => undefined);
    await Promise.resolve();
    await Promise.resolve();
    retirePaneInput('gate:retire');
    release();
    await first;
    await expect(queued).rejects.toBeInstanceOf(PaneInputGateRetiredError);
  });

  it('rejects a bounded burst without dropping admitted work', async () => {
    let release!: () => void;
    const wait = new Promise<void>((resolve) => { release = resolve; });
    const first = enqueuePaneInput('gate:bounded', async () => { await wait; }, { maxPending: 2 });
    const second = enqueuePaneInput('gate:bounded', () => undefined, { maxPending: 2 });
    expect(tryEnqueuePaneInput('gate:bounded', () => undefined, { maxPending: 2 })).toBe(false);
    release();
    await Promise.all([first, second]);
  });
});
