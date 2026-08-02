import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const shared = readFileSync(new URL('./SidebarGitPanel.svelte', import.meta.url), 'utf8');
const remote = readFileSync(new URL('../../remote/lib/RemoteGitPanel.svelte', import.meta.url), 'utf8');

describe('git sidebar request lifecycle guards', () => {
  it('fences shared Git status responses and aborts on destroy', () => {
    expect(shared).toContain('provider.gitStatus(controller.signal)');
    expect(shared).toContain('generation !== requestGeneration');
    expect(shared).toContain('requestController?.abort()');
    expect(shared).toContain('onDestroy');
  });

  it('fences Remote Git status responses and cancels action on destroy', () => {
    expect(remote).toContain('provider.gitStatus(controller.signal)');
    expect(remote).toContain('generation !== loadGeneration');
    expect(remote).toContain('loadController?.abort()');
    expect(remote).toContain('actionController?.abort()');
    expect(remote).toContain('onDestroy');
  });
});
