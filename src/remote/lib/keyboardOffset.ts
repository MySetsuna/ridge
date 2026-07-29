export interface TerminalVisualShiftInput {
  layoutHeightPx: number;
  visualHeightPx: number;
  visualOffsetTopPx: number;
  stageTopPx: number;
  cursorYPx: number;
  cellHeightPx: number;
  contextRows?: number;
}

export interface InputAnchorPoint {
  x: number;
  y: number;
  h: number;
}

export interface InputAnchorBounds {
  containerLeft: number;
  containerTop: number;
  containerWidth: number;
  containerHeight: number;
  visualLeft: number;
  visualTop: number;
  visualWidth: number;
  visualHeight: number;
}

/** Cursor-only IME anchor. Invalid/off-screen cursors fall back to the centre
 * of the terminal area that is actually visible through VisualViewport. */
export function resolveInputAnchor(
  cursor: InputAnchorPoint | null,
  bounds: InputAnchorBounds,
): InputAnchorPoint {
  const width = Math.max(1, bounds.containerWidth);
  const height = Math.max(1, bounds.containerHeight);
  if (
    cursor
    && Number.isFinite(cursor.x)
    && Number.isFinite(cursor.y)
    && cursor.x >= 0
    && cursor.x < width
    && cursor.y >= 0
    && cursor.y < height
  ) {
    return {
      x: cursor.x,
      y: cursor.y,
      h: Number.isFinite(cursor.h) && cursor.h > 0 ? cursor.h : 1,
    };
  }

  const visibleLeft = Math.max(bounds.containerLeft, bounds.visualLeft);
  const visibleTop = Math.max(bounds.containerTop, bounds.visualTop);
  const visibleRight = Math.min(
    bounds.containerLeft + width,
    bounds.visualLeft + Math.max(1, bounds.visualWidth),
  );
  const visibleBottom = Math.min(
    bounds.containerTop + height,
    bounds.visualTop + Math.max(1, bounds.visualHeight),
  );
  const centerX = visibleRight > visibleLeft
    ? (visibleLeft + visibleRight) / 2 - bounds.containerLeft
    : width / 2;
  const centerY = visibleBottom > visibleTop
    ? (visibleTop + visibleBottom) / 2 - bounds.containerTop
    : height / 2;
  return {
    x: Math.max(0, Math.min(width - 1, centerX)),
    y: Math.max(0, Math.min(height - 1, centerY)),
    h: 1,
  };
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
