import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('cross-surface interaction contracts', () => {
  it('keeps workspace sharing reachable from the workspace tab menu', () => {
    const tabs = read('./WorkspaceTabs.svelte');
    expect(tabs).toContain("id: 'share'");
    expect(tabs).toContain('分享工作区');
    expect(tabs).toContain('onShare?.(');
  });

  it('keeps desktop IME punctuation on the same PTY write path', () => {
    const pane = read('./RidgePane.svelte');
    expect(pane).toContain('function onImeHelperInput');
    expect(pane).toContain('oninput={onImeHelperInput}');
    expect(pane).toContain('IME_INPUT_DEDUP_WINDOW_MS');
  });

  it('keeps host attach progress outside the connect dialog', () => {
    const dialog = read('./hosts/HostConnectDialog.svelte');
    const panel = read('./hosts/HostsPanel.svelte');
    expect(dialog).toContain('hostConnectProgress');
    expect(dialog.indexOf('close();')).toBeLessThan(dialog.indexOf('connectHost('));
    expect(panel).toContain('$hostConnectProgress');
    expect(panel).toContain('onAttach');
    expect(panel).toContain('hostSessionDrag');
  });

  it('keeps narrow-screen file output inside the viewport', () => {
    const viewer = read('../../remote/lib/FileViewer.svelte');
    expect(viewer).toContain('overflow-x: hidden');
    expect(viewer).toContain('overflow-wrap: anywhere');
    expect(viewer).not.toContain('min-width: max-content');
    expect(viewer).toContain('const imagePreview = $derived');
    expect(viewer).toContain('class="image-preview"');
    const image = read('./ImagePreviewOverlay.svelte');
    expect(image).toContain('max-width: min(92vw, calc(100vw - 16px))');
    expect(image).toContain('safe-area-inset-bottom');
  });

  it('keeps Remote Agent controls touch-sized and narrow-safe', () => {
    const remote = read('../../remote/lib/SidebarTeamRoster.svelte');
    expect(remote).toContain('min-height:32px');
    expect(remote).toContain('min-height:40px');
    expect(remote).toContain('white-space:nowrap');
  });

  it('keeps Agent history tabs icon-free and CWD-visible on desktop and Remote', () => {
    const desktop = read('../teammate/AgentCenterPanel.svelte');
    const remote = read('../../remote/lib/SidebarTeamRoster.svelte');
    expect(desktop).toContain('历史');
    expect(remote).toContain('History {history.length}');
    expect(desktop).not.toContain('<History ');
    expect(remote).not.toContain('<History ');
    expect(remote).toContain('cwd');
  });

  it('releases desktop TUI mouse capture on pointer cancellation', () => {
    const manager = read('../../../packages/remote/src/shared/terminal/manager.ts');
    expect(manager).toContain('const pointerUpListener = (event: PointerEvent, force = false) =>');
    expect(manager).toContain('if (!force && isScrollbar(event)) return;');
    expect(manager).toContain('const pointerCancelListener = (event: PointerEvent) => pointerUpListener(event, true);');
    expect(manager).toContain("container.addEventListener('pointercancel', pointerCancelListener)");
    expect(manager).toContain("entry.container.removeEventListener('pointercancel', entry.pointerCancelListener)");
  });
});
