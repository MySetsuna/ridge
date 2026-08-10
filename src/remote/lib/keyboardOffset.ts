export interface TerminalVisualShiftInput {
  layoutHeightPx: number;
  visualHeightPx: number;
  visualOffsetTopPx: number;
  stageTopPx: number;
  cursorYPx: number;
  cellHeightPx: number;
  contextRows?: number;
  /** Optional viewport-space input rect. Defaults to cursor + one cell. */
  inputTopPx?: number;
  inputBottomPx?: number;
  /** Keyboard top in viewport coordinates. Defaults to offsetTop + height. */
  keyboardTopPx?: number;
  /** Extra breathing room below the input; defaults to one cell. */
  safeGapPx?: number;
  /** Previous visual shift, used for bounded convergence and hysteresis. */
  previousShiftPx?: number;
  hysteresisPx?: number;
  maxStepPx?: number;
  maxShiftPx?: number;
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
  inputTopPx,
  inputBottomPx,
  keyboardTopPx,
  safeGapPx,
  previousShiftPx,
  hysteresisPx = 0,
  maxStepPx = Number.POSITIVE_INFINITY,
  maxShiftPx = Number.POSITIVE_INFINITY,
}: TerminalVisualShiftInput): number {
  const previous = Number.isFinite(previousShiftPx) ? previousShiftPx! : undefined;
  if (
    !Number.isFinite(layoutHeightPx)
    || !Number.isFinite(visualHeightPx)
    || !Number.isFinite(visualOffsetTopPx)
    || !Number.isFinite(stageTopPx)
    || !Number.isFinite(cursorYPx)
     || cellHeightPx <= 0
  ) return previous ?? 0;

  const viewportKeyboardTop = Number.isFinite(keyboardTopPx)
    ? keyboardTopPx!
    : visualOffsetTopPx + visualHeightPx;
  const keyboardVisible = viewportKeyboardTop < layoutHeightPx - 1;
  if (!keyboardVisible) {
    return stabilizeTerminalVisualShiftPx(0, previous, { hysteresisPx, maxStepPx });
  }

  const inputTop = Number.isFinite(inputTopPx)
    ? inputTopPx!
    : stageTopPx + cursorYPx;
  const measuredInputBottom = Number.isFinite(inputBottomPx)
    ? inputBottomPx!
    : inputTop + cellHeightPx;
  const inputBottom = Math.max(inputTop, measuredInputBottom);
  const safeGap = Number.isFinite(safeGapPx) && safeGapPx! >= 0
    ? safeGapPx!
    : cellHeightPx;
  const desiredBottom = viewportKeyboardTop - safeGap;
  const needed = Math.min(0, Math.round(desiredBottom - inputBottom));
  const context = Number.isFinite(contextRows) && contextRows >= 0 ? contextRows : 3;
  const contextPx = Math.max(cellHeightPx, context * cellHeightPx);
  const cursorTopInStage = inputTop - stageTopPx;
  const maxUp = Math.max(0, Math.round(cursorTopInStage - contextPx));
  const maxShift = Number.isFinite(maxShiftPx) && maxShiftPx! >= 0 ? maxShiftPx! : Number.POSITIVE_INFINITY;
  const bounded = Math.max(-maxUp, Math.max(-maxShift, needed));
  return stabilizeTerminalVisualShiftPx(bounded, previous, { hysteresisPx, maxStepPx });
}

export interface VisualShiftStabilizationOptions {
  hysteresisPx?: number;
  maxStepPx?: number;
}

/**
 * Apply a target visual shift without letting viewport jitter move the stage by
 * sub-pixel noise or jump an entire terminal in one frame. Pure and deterministic
 * so callers can replay keyboard open/close geometry in tests.
 */
export function stabilizeTerminalVisualShiftPx(
  targetPx: number,
  previousPx: number | undefined,
  { hysteresisPx = 0, maxStepPx = Number.POSITIVE_INFINITY }: VisualShiftStabilizationOptions = {},
): number {
  if (!Number.isFinite(targetPx)) return Number.isFinite(previousPx) ? previousPx! : 0;
  if (!Number.isFinite(previousPx)) return Math.round(targetPx);
  const hysteresis = Math.max(0, Number.isFinite(hysteresisPx) ? hysteresisPx : 0);
  // Never leave a residual negative transform after the keyboard closes. The
  // hysteresis band is only for two non-zero keyboard positions; target zero is
  // an explicit release, even when the previous shift is just a few pixels.
  const hold = targetPx !== 0
    && previousPx! !== 0
    && Math.abs(targetPx - previousPx!) <= hysteresis;
  const target = hold ? previousPx! : targetPx;
  const maxStep = Number.isFinite(maxStepPx) && maxStepPx! > 0
    ? maxStepPx!
    : Number.POSITIVE_INFINITY;
  const delta = target - previousPx!;
  if (Math.abs(delta) <= maxStep) return Math.round(target);
  return Math.round(previousPx! + Math.sign(delta) * maxStep);
}
