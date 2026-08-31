import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./MainApp.svelte', import.meta.url), 'utf8');

describe('remote Agent attention monitor', () => {
  it('keeps live roster attention polling while the drawer is closed', () => {
    expect(source).toContain("const RemoteTeamRoster = import('./lib/SidebarTeamRoster.svelte');");
    expect(source).toContain("ui.sidebarTab !== 'team'");
    expect(source).toContain('class="agent-attention-monitor"');
    expect(source).toContain('onAttentionChange={updateAgentAttention}');
  });

  it('routes mobile terminal paths through the file viewer without opening the keyboard', () => {
    expect(source).toContain("ridge:remote-open-text-link");
    expect(source).toContain('openFileViewer(path, line);');
    expect(source).toContain('remotePerfStart(\'pane-switch\'');
    expect(source).toContain('onFirstPaint={markPaneFirstPaint}');
  });

  it('frame-budgets pane output so input events keep the main thread turn', () => {
    expect(source).toContain("import { PaneFeedScheduler } from './lib/paneFeedScheduler';");
    expect(source).toContain('paneFeedScheduler.enqueue(key, data);');
    expect(source).toContain('paneFeedScheduler.setActive(subscriptionKey);');
    expect(source).toContain('paneFeedScheduler.clearAll();');
  });

  it('marks browser-driven pane fits as remote-owned claims', () => {
    expect(source).toContain("return ws.claimPane(pane, rows, cols, pixelWidth, pixelHeight, 'remote');");
  });

  it('settles current mobile geometry before refresh and atomically prepends history', () => {
    expect(source).toContain('async function handleRefresh()');
    expect(source).toContain('await canvasRef.claimPaneSize();');
    expect(source).toContain('page.commit(() => targetCanvas.prependScrollbackForPane(key, bytes))');
  });

  it('releases every pane-owned queue, timer, trace, transport, and kernel', () => {
    const start = source.indexOf('function releasePaneRuntime');
    const end = source.indexOf('\n  }\n\n  // Free kernels', start);
    const release = source.slice(start, end);
    expect(release).toContain('attachedPanes.delete(key);');
    expect(release).toContain('paneFeedScheduler.clear(key);');
    expect(release).toContain('pendingRawFrames.drop(key);');
    expect(release).toContain('paneSwitchPerf.delete(key);');
    expect(release).toContain('replayedPanes.delete(key);');
    expect(release).toContain('clearFeedResync(key);');
    expect(release).toContain('canvasRef?.clearPendingFeed(key);');
    expect(source).toContain('if (!remoteAppAlive || !feedResyncPending.has(key)) return;');
    expect(source).toContain('for (const pane of ownedPanes) releasePaneRuntime(pane);');
    expect(source).toContain('ws.pruneOutputs(new Set());');
    expect(source).toContain('void detachPaneKernels(ownedPanes);');
  });
});
