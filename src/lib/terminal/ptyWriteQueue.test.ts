import { describe, expect, it } from 'vitest';
import { enqueuePtyWrite } from './ptyWriteQueue';

describe('enqueuePtyWrite', () => {
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
});
