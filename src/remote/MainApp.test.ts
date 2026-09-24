import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./MainApp.svelte', import.meta.url), 'utf8');

describe('remote Agent attention monitor', () => {
  it('keeps live roster attention polling while the drawer is closed', () => {
    expect(source).toContain("remoteTeamRosterPromise ??= import('./lib/SidebarTeamRoster.svelte');");
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

  it('settles current mobile geometry, drops stale frames, and requests a canonical screen on refresh', () => {
    expect(source).toContain('async function handleRefresh()');
    const start = source.indexOf('async function handleRefresh()');
    const end = source.indexOf('\n  }\n\n  let _refreshSeq', start);
    const refresh = source.slice(start, end);
    expect(refresh.indexOf('await canvasRef.claimPaneSize();')).toBeLessThan(refresh.indexOf('paneFeedScheduler.clear(key);'));
    expect(refresh).toContain('pendingRawFrames.drop(key);');
    expect(refresh).toContain('canvasRef?.clearPendingFeed(key);');
    expect(refresh).toContain('clearFeedResync(key);');
    expect(refresh).toContain('ws.resyncPane?.(pane);');
  });

  it('atomically prepends lazy scrollback pages', () => {
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

  it('cold boot: buffers every pane frame into its own queue before the active binding lands, never drops pre-active bytes, never cross-pane bleeds', () => {
    // 冷启动时 ui.activeWorkspaceId/activePaneId 都未确定，host 先推的 binary
    // PTY bytes 必须进对应 pane 的 PaneFeedScheduler 队列；active binding 一旦
    // 由 refreshWorkspaces / panes handler 落定，queue 自动 drain 到 active canvas。
    // active 已确立且 key 不匹配 → drop straggler（行为不变）。
    expect(source).toContain('paneFeedScheduler.enqueue(key, data);');
    // 新 gate：active 未设时不再 return，frames 入 own pane queue。
    expect(source).toContain('if (active && key !== paneRefKey(active)) return;');
    // refreshWorkspaces 是 activeWorkspaceId+activePaneId 的唯一 owner。
    expect(source).toContain('async function refreshWorkspaces()');
    expect(source).toContain('ui.navigate(restoredWorkspaceId, restoredPaneId);');
    expect(source).toContain('bootRestoreDone = true;');
    // savedActiveWs 不再独立 pre-seed（消除双写）。restore 决策全在 refreshWorkspaces。
    expect(source).not.toMatch(/if\s*\(\s*savedActiveWs\s*\)\s*\{\s*\n\s*ui\.navigate\(savedActiveWs,/);
    // gate 不再用"activeWorkspaceId 为空就放宽"的写法。
    expect(source).not.toMatch(/workspaceId\s*!==\s*ui\.activeWorkspaceId\s*\)\s*return;\s*\n\s*\/\/\s*放宽冷启动/);
    // panes handler 决策不因 activeWorkspaceId 为空而短路（cold boot 时仍是 active binding 的 source of truth）。
    expect(source).not.toMatch(/activeWorkspaceId\s*===\s*''\s*\?\s*null\s*:/);
  });

  it('scopes lightweight navigation preferences to the connected host', () => {
    expect(source).toContain('ws.cacheScope?.()');
    expect(source).toContain('`rg-remote-active-ws:${storageScope}`');
    expect(source).toContain('`rg-remote-pane-map:${storageScope}`');
  });
});
