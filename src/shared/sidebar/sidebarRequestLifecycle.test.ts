import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const fileTree = readFileSync(new URL('./SidebarFileTree.svelte', import.meta.url), 'utf8');
const search = readFileSync(new URL('./SidebarSearch.svelte', import.meta.url), 'utf8');
const viewer = readFileSync(new URL('../../remote/lib/FileViewer.svelte', import.meta.url), 'utf8');

describe('shared sidebar request lifecycle guards', () => {
  it('cancels and fences directory responses', () => {
    expect(fileTree).toContain('requestController?.abort()');
    expect(fileTree).toContain('provider.listDir(target, controller.signal)');
    expect(fileTree).toContain('generation !== requestGeneration');
    expect(fileTree).toContain('onDestroy');
  });

  it('cancels and fences search responses', () => {
    expect(search).toContain('provider.search(q, controller.signal)');
    expect(search).toContain('generation !== requestGeneration');
    expect(search).toContain('clearTimeout(debounce)');
    expect(search).toContain('onDestroy');
  });

  it('cancels and fences file and diff viewer responses', () => {
    expect(viewer).toContain('provider.gitDiff(path, controller.signal)');
    expect(viewer).toContain('provider.readFile(path, controller.signal)');
    expect(viewer).toContain('generation !== loadGeneration');
    expect(viewer).toContain('onDestroy');
  });
});
