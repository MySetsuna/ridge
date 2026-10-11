import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./MainApp.svelte', import.meta.url), 'utf8');

describe('remote Agent attention monitor', () => {
  it('keeps live roster attention polling while the drawer is closed', () => {
    expect(source).toContain("retryableLazyImport(() => import('./lib/SidebarTeamRoster.svelte'))");
    expect(source).toContain('remoteTeamRosterPromise ??= loadRemoteTeamRoster();');
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

  it('bounds canonical packets during mount gaps and recovers on overflow', () => {
    expect(source).toContain('ws.onTerminalFrame((data) => {');
    expect(source).toContain('queued.length >= 64 || queuedBytes + data.byteLength > terminalHello.maxFrameBytes');
    expect(source).toContain('requestFeedResync(key);');
    expect(source).not.toContain('ws.onRawBytes(');
  });

  it('marks browser-driven pane fits as remote-owned claims', () => {
    expect(source).toContain("return ws.claimPane(pane, rows, cols, pixelWidth, pixelHeight, 'remote');");
  });

  it('settles current mobile geometry, drops stale frames, and requests a canonical screen on refresh', () => {
    expect(source).toContain('async function handleRefresh()');
    const start = source.indexOf('async function handleRefresh()');
    const end = source.indexOf('\n  }\n\n  let _refreshSeq', start);
    const refresh = source.slice(start, end);
    expect(refresh.indexOf('await canvasRef.claimPaneSize();')).toBeLessThan(refresh.indexOf('pendingSemanticFrames.delete(key);'));
    expect(refresh).toContain('pendingSemanticFrames.delete(key);');
    expect(refresh).toContain('canvasRef?.clearPendingFeed(key);');
    expect(refresh).toContain('clearFeedResync(key);');
    expect(refresh).toContain('ws.resyncPane?.(pane);');
  });

  it('atomically prepends lazy scrollback pages', () => {
    expect(source).toContain('const request = target.terminalHistoryRequest();');
    expect(source).toContain('await ws.fetchTerminalHistory(request)');
    expect(source).not.toContain('scrollbackDecoder.decode');
  });

  it('releases every pane-owned queue, timer, trace, transport, and kernel', () => {
    const start = source.indexOf('function releasePaneRuntime');
    const end = source.indexOf('\n  }\n\n  // Free kernels', start);
    const release = source.slice(start, end);
    expect(release).toContain('attachedPanes.delete(key);');
    expect(release).toContain('pendingSemanticFrames.delete(key);');
    expect(release).toContain('paneSwitchPerf.delete(key);');
    expect(release).toContain('clearFeedResync(key);');
    expect(release).toContain('canvasRef?.clearPendingFeed(key);');
    expect(source).toContain('if (!remoteAppAlive || !feedResyncPending.has(key)) return;');
    expect(source).toContain('for (const pane of ownedPanes) releasePaneRuntime(pane);');
    expect(source).toContain('ws.pruneOutputs(new Set());');
    expect(source).toContain('void detachPaneKernels(ownedPanes);');
  });

  it('streams only the atomic active pane and drops late frames from the old pane', () => {
    expect(source).toContain('(ws.activatePane ?? ws.subscribePane).call(ws, pane, {');
    expect(source).toContain('const activationId = nextTerminalActivation(subscriptionKey);');
    expect(source).toContain('activationId,');
    expect(source).not.toContain('ws.subscribePane(pane, { active: false });');
    expect(source).toContain('ui.navigate(workspaceId, remembered);');
  });

  it('subscribes only after the authoritative active binding lands', () => {
    expect(source).toContain('if (!pane) return;');
    expect(source).toContain('if (!activation || activation.key !== key) return;');
    expect(source).toContain('async function refreshWorkspaces()');
    expect(source).toContain('ui.navigate(restoredWorkspaceId, restoredPaneId);');
    expect(source).toContain('bootRestoreDone = true;');
    expect(source).not.toMatch(/if\s*\(\s*savedActiveWs\s*\)\s*\{\s*\n\s*ui\.navigate\(savedActiveWs,/);
  });

  it('scopes lightweight navigation preferences to the connected host', () => {
    expect(source).toContain('ws.cacheScope?.()');
    expect(source).toContain('`rg-remote-active-ws:${storageScope}`');
    expect(source).toContain('`rg-remote-pane-map:${storageScope}`');
  });
});
