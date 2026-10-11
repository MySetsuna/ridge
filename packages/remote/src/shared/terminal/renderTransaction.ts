/** Explicit VT synchronized-output transactions are semantic frame boundaries,
 * so defer their presentation without guessing from a timer. The timeout is
 * only a safety valve for a broken client that never sends `?2026l`. */
export const SYNC_OUTPUT_TIMEOUT_MS = 150;
