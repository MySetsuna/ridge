import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const mobileApp = readFileSync(new URL('../../remote/MainApp.svelte', import.meta.url), 'utf8');
const canvas = readFileSync(new URL('../../remote/lib/TerminalCanvas.svelte', import.meta.url), 'utf8');
const desktopPane = readFileSync(new URL('../components/RidgePane.svelte', import.meta.url), 'utf8');
const paneSizeSync = readFileSync(new URL('./paneSizeSync.ts', import.meta.url), 'utf8');
const manager = readFileSync(new URL('../../../packages/remote/src/shared/terminal/manager.ts', import.meta.url), 'utf8');
const wsRemote = readFileSync(new URL('../../../packages/remote/src/shared/transport/wsRemote.ts', import.meta.url), 'utf8');

describe('remote refresh entrypoints share one forced remount path', () => {
  it('phone lock-and-refresh claims the measured pane instead of a same-size fit no-op', () => {
    expect(mobileApp).toContain('function handleRefresh()');
    expect(mobileApp).toContain('canvasRef.claimPaneSize()');
    expect(mobileApp).not.toMatch(/function handleRefresh\(\)[\s\S]{0,240}canvasRef\.fitPaneNow\(\)/);
    expect(canvas).toContain('export function claimPaneSize()');
    expect(canvas).toContain('manager.claimPaneSize(paneId)');
    expect(canvas).toContain('manager.forceFullRedraw(paneId)');
  });

  it('PC browser pane refresh uses the same claim-and-redraw helper', () => {
    expect(desktopPane).toContain('function refreshForRemote()');
    expect(desktopPane).toContain('synchronizePaneSize(paneId)');
    expect(desktopPane).toContain('scheduleForcedPaneResize(');
    expect(paneSizeSync).toContain('manager.claimPaneSize(paneId)');
    expect(paneSizeSync).toContain('manager.forceFullRedraw(paneId)');
  });

  it('explicit claim remounts host even when the measured grid is unchanged', () => {
    expect(manager).toContain('void this.fitPane(entry, true, true)');
    expect(manager).toContain('if (!this._fitGridChanged(entry, grid) && !force) return');
    expect(wsRemote).toContain('scheduleResize(pane, rows, cols, undefined, { force: true })');
  });

  it('pty-resized recovery still consumes a full local redraw', () => {
    expect(canvas).toContain('export function resizeKernel(rows: number, cols: number)');
    expect(canvas).toContain('manager.forceFullRedraw(paneId)');
    expect(mobileApp).toContain('canvasRef?.resizeKernel(rows, cols)');
  });
});
