/** Native-parser repaint transactions share this quiet window across the main
 * compositor and optional worker renderer. Grid paints never wait for it;
 * only the transient terminal cursor stays hidden until a redraw walk stops. */
export const TUI_CURSOR_SETTLE_MS = 24;

/** Explicit VT synchronized-output transactions are semantic frame boundaries,
 * so defer their presentation without guessing from a timer. The timeout is
 * only a safety valve for a broken client that never sends `?2026l`. */
export const SYNC_OUTPUT_TIMEOUT_MS = 150;
