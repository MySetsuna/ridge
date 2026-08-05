/**
 * Ordered DataChannel buffering budgets shared by host and controller. Keep
 * this module dependency-free so the mobile controller does not pull the host
 * bridge into its initial bundle just to read two constants.
 */
// A single ordered DataChannel must leave a small latency budget for control
// and terminal input; waiting until multi-megabyte SCTP buffering is too late.
export const BUFFERED_LOW_WATERMARK = 64 * 1024;
export const BUFFERED_HIGH_WATERMARK = 256 * 1024;
