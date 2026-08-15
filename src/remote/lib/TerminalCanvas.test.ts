import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./TerminalCanvas.svelte', import.meta.url), 'utf8');

describe('remote pane Agent status chrome contract', () => {
  it('uses only a transient intervention rail without changing terminal geometry', () => {
    expect(source).toContain('class:agent-needs-attention={agentNeedsAttention}');
    expect(source).toContain('.container.agent-needs-attention{box-shadow:inset 0 0 0 2px var(--rg-ansi-yellow,#d29922)}');
    expect(source).not.toContain('.container.agent-working{');
    expect(source).not.toContain('.container.agent-starting{');
    expect(source).not.toContain('.container.agent-idle{');
  });

  it('clears the transient rail when the pane input surface receives focus', () => {
    expect(source).toContain('onFocus: onPaneFocus');
    expect(source).toContain('onPaneFocus?.(pane)');
    expect(source).toContain('manager.setFocused(paneId, true)');
  });

  it('keeps switch-gap frames and first keystrokes instead of dropping them', () => {
    expect(source).toContain('onDrainPending');
    expect(source).toContain('const pendingFrames = onDrainPending?.(paneId) ?? []');
    expect(source).toContain('const MAX_PENDING_STDIN_BYTES = 64 * 1024;');
    expect(source).toContain('focusInput();');
    expect(source).toMatch(/if \(!attached\) \{\r?\n\s+onStdin\(text\);/);
  });

  it('keeps renderer cursor ownership when the mobile IME sink blurs', () => {
    expect(source).toContain('manager.setFocused(paneId, true);');
    expect(source).not.toContain('onblur={() => manager.setFocused(paneId, false)}');
    expect(source).toContain('caret-color:var(--rg-accent,#58a6ff)');
  });

  it('keeps pane geometry authoritative for host resize recovery', () => {
    expect(source).toContain('export function fitPaneNow()');
    expect(source).toContain('if (attached) manager.fitPaneNow(paneId);');
    expect(source).toContain('export function claimPaneSize()');
    expect(source).toContain('manager.claimPaneSize(paneId)');
    expect(source).toContain('manager.forceFullRedraw(paneId)');
    expect(source).toContain('export function resizeKernel(_rows: number, _cols: number)');
    expect(source).toContain('manager.forceFullRedraw(paneId)');
    expect(source).not.toContain('manager.getKernel(paneId)?.resize(rows, cols);');
  });

  it('does not drop mobile spaces reported as insertCompositionText', () => {
    expect(source).toContain('Some mobile keyboards');
    expect(source).toContain('if (text === imeCommitExpect && Date.now() - imeCommitExpectTime < IME_DUP_WINDOW_MS)');
    expect(source).not.toContain("if (inputType === 'insertCompositionText') return;");
  });

  it('forwards touch press-drag-release to mouse-reporting TUIs', () => {
    expect(source).toContain('let touchMouseDragging = false;');
    expect(source).toContain("decideTouchMouseGesture('press')");
    expect(source).toContain("decideTouchMouseGesture('drag')");
    expect(source).toContain("decideTouchMouseGesture('release')");
    expect(source).toContain('ontouchcancel={handleTouchCancel}');
  });

  it('requires the platform modifier for mouse links while preserving touch taps', () => {
    expect(source).toContain('e.button === 0 && linkModifierHeld(e) && manager.openLinkAt(paneId, cell.row, cell.col)');
    expect(source).toContain('return isMac ? e.metaKey : e.ctrlKey;');
    expect(source).toContain('const linkCell = touchLinkCell;');
    expect(source).toMatch(/e\.preventDefault\(\);\r?\n\s+return;/);
  });

  it('reports the first post-switch frame without retaining payloads', () => {
    expect(source).toContain('onFirstPaint?: (paneKey: string) => void;');
    expect(source).toContain('requestAnimationFrame(() => {');
    expect(source).toContain('onFirstPaint?.(paneId);');
  });
});
