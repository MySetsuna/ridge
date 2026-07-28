export interface TerminalVisualShiftInput {
  layoutHeightPx: number;
  visualHeightPx: number;
  visualOffsetTopPx: number;
  stageTopPx: number;
  cursorYPx: number;
  cellHeightPx: number;
  contextRows?: number;
}

/**
 * Bounded visual-only keyboard avoidance. Negative result translates the whole
 * terminal stage; layout, canvas pixels and PTY grid remain untouched.
 */
export function terminalVisualShiftPx({
  layoutHeightPx,
  visualHeightPx,
  visualOffsetTopPx,
  stageTopPx,
  cursorYPx,
  cellHeightPx,
  contextRows = 3,
}: TerminalVisualShiftInput): number {
  if (
    !Number.isFinite(layoutHeightPx)
    || !Number.isFinite(visualHeightPx)
    || !Number.isFinite(visualOffsetTopPx)
    || !Number.isFinite(stageTopPx)
    || !Number.isFinite(cursorYPx)
    || !(cellHeightPx > 0)
    || layoutHeightPx - visualHeightPx <= 0
  ) return 0;

  const visibleBottom = visualOffsetTopPx + visualHeightPx;
  const cursorBottom = stageTopPx + cursorYPx + cellHeightPx;
  const desiredBottom = visibleBottom - cellHeightPx;
  const needed = Math.min(0, Math.round(desiredBottom - cursorBottom));
  const contextPx = Math.max(cellHeightPx, contextRows * cellHeightPx);
  const maxUp = Math.max(0, Math.round(cursorYPx - contextPx));
  return Math.max(-maxUp, needed);
}
