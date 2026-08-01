import { TerminalManager } from '@ridge/remote/shared/terminal/manager';

/** One authoritative path for automatic post-attach sizing and the Resize button. */
export function synchronizePaneSize(paneId: string): boolean {
  const manager = TerminalManager.tryInstance();
  if (!manager) return false;
  if (manager.rows(paneId) <= 0 || manager.cols(paneId) <= 0) return false;
  manager.claimPaneSize(paneId);
  manager.forceFullRedraw(paneId);
  return true;
}

/** Wait across mount/layout readiness, then submit exactly one verified claim. */
export function schedulePaneSizeSynchronization(paneId: string): void {
  if (typeof requestAnimationFrame === 'undefined') {
    synchronizePaneSize(paneId);
    return;
  }
  let synchronized = false;
  const synchronize = () => {
    if (!synchronized) synchronized = synchronizePaneSize(paneId);
  };
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      synchronize();
      if (synchronized) return;
      setTimeout(synchronize, 50);
      setTimeout(synchronize, 150);
      setTimeout(synchronize, 400);
    });
  });
}
