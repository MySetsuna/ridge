import type { PaneRpcScheduler } from '@ridge/remote/shared/transport/paneRpcScheduler';
import type { PaneRef } from '@ridge/remote/shared/transport/paneRef';

/** Desktop / WEB_REMOTE host resize after fit already decided to apply.
 *  Explicit refresh must remount even when rows×cols match the last claim. */
export function scheduleForcedPaneResize(
  scheduler: Pick<PaneRpcScheduler, 'scheduleResizeAndWait'>,
  pane: PaneRef,
  rows: number,
  cols: number,
  params?: Readonly<Record<string, unknown>>,
): Promise<void> {
  return scheduler.scheduleResizeAndWait(pane, rows, cols, params, { force: true });
}
