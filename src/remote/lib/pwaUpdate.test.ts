import { get } from 'svelte/store';
import { beforeEach, describe, expect, it, vi } from 'vitest';

let updates: typeof import('./pwaUpdate');
beforeEach(async () => { vi.resetModules(); updates = await import('./pwaUpdate'); });

function editor() {
  const state = { dirty: true, saving: false };
  const save = vi.fn(async () => { state.dirty = false; });
  const discard = vi.fn(() => { state.dirty = false; });
  const remove = updates.registerPwaEditGuard({ dirty: () => state.dirty, saving: () => state.saving, save, discard });
  return { state, save, discard, remove };
}

describe('PWA live editor update gate', () => {
  it('blocks automatic reload with unsaved edits and applies only after the live guard clears', async () => {
    const apply = vi.fn(async () => {});
    updates.configurePwaUpdate(apply); updates.markPwaUpdateReady();
    const edit = editor();
    expect(await updates.applyPwaUpdate()).toBe(false);
    expect(get(updates.pwaUpdateReady)).toBe(true);
    expect(apply).not.toHaveBeenCalled();
    edit.state.dirty = false;
    expect(await updates.applyPwaUpdate()).toBe(true);
    expect(apply).toHaveBeenCalledExactlyOnceWith(true);
  });

  it('blocks save/discard/update while an existing file save is in flight', async () => {
    const apply = vi.fn(async () => {});
    updates.configurePwaUpdate(apply); updates.markPwaUpdateReady();
    const edit = editor(); edit.state.saving = true;
    await updates.applyPwaUpdate('save'); await updates.applyPwaUpdate('discard');
    expect(edit.save).not.toHaveBeenCalled();
    expect(edit.discard).not.toHaveBeenCalled();
    expect(apply).not.toHaveBeenCalled();
    edit.state.saving = false;
    expect(await updates.applyPwaUpdate('save')).toBe(true);
  });

  it('keeps all remaining drafts and update readiness on a partial save failure', async () => {
    const apply = vi.fn(async () => {});
    updates.configurePwaUpdate(apply); updates.markPwaUpdateReady();
    const first = editor(), second = editor();
    second.save.mockRejectedValueOnce(new Error('offline'));
    expect(await updates.applyPwaUpdate('save')).toBe(false);
    expect(first.state.dirty).toBe(false);
    expect(second.state.dirty).toBe(true);
    expect(get(updates.pwaUpdateError)).toBe('offline');
    expect(get(updates.pwaUpdateReady)).toBe(true);
    expect(get(updates.pwaUpdateBusy)).toBe(false);
    expect(apply).not.toHaveBeenCalled();
    expect(await updates.applyPwaUpdate('save')).toBe(true);
  });

  it('rechecks all live guards after saving and refuses a newly dirty editor', async () => {
    const apply = vi.fn(async () => {});
    updates.configurePwaUpdate(apply); updates.markPwaUpdateReady();
    const first = editor();
    first.save.mockImplementationOnce(async () => { first.state.dirty = false; editor(); });
    expect(await updates.applyPwaUpdate('save')).toBe(false);
    expect(apply).not.toHaveBeenCalled();
  });

  it('requires explicit discard and keeps a retry entry if the updater fails', async () => {
    const apply = vi.fn(async () => {});
    apply.mockRejectedValueOnce(new Error('worker unavailable'));
    updates.configurePwaUpdate(apply); updates.markPwaUpdateReady();
    const edit = editor();
    expect(await updates.applyPwaUpdate('discard')).toBe(false);
    expect(edit.discard).toHaveBeenCalledOnce();
    expect(edit.save).not.toHaveBeenCalled();
    expect(get(updates.pwaUpdateReady)).toBe(true);
    expect(get(updates.pwaUpdateError)).toBe('worker unavailable');
    expect(await updates.applyPwaUpdate()).toBe(true);
  });

  it('honors Later across background calls and releases destroyed editor guards', async () => {
    const apply = vi.fn(async () => {});
    updates.configurePwaUpdate(apply); updates.markPwaUpdateReady();
    const edit = editor(); edit.remove();
    expect(get(updates.pwaUpdateBlocked)).toBe(false);
    updates.deferPwaUpdate();
    expect(await updates.applyPwaUpdate()).toBe(false);
    updates.resumePwaUpdatePrompt();
    expect(await updates.applyPwaUpdate()).toBe(true);
  });

  it('serializes repeated update actions and retains a newer update notification', async () => {
    let finish!: () => void;
    const apply = vi.fn(() => new Promise<void>(resolve => { finish = resolve; }));
    updates.configurePwaUpdate(apply); updates.markPwaUpdateReady();
    const pending = updates.applyPwaUpdate();
    expect(await updates.applyPwaUpdate('discard')).toBe(false);
    updates.markPwaUpdateReady(); finish();
    expect(await pending).toBe(true);
    expect(apply).toHaveBeenCalledOnce();
    expect(get(updates.pwaUpdateReady)).toBe(true);
  });
});
