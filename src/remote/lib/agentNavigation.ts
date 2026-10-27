import { confirmedWorkspaceTarget } from './remoteQueries';

/** Serialize Host selection and commit only the latest Agent click. */
export function createAgentNavigator(input: {
  switchWorkspace: (id: string) => Promise<boolean>;
  navigate: (workspaceId: string, paneId: string) => void;
  isAlive: () => boolean;
  onError: (error: unknown) => void;
  failureMessage: string;
}) {
  let generation = 0;
  let queue = Promise.resolve();
  return (workspaceId: string, paneId: string): Promise<void> => {
    const version = ++generation;
    queue = queue.then(async () => {
      if (version !== generation || !input.isAlive()) return;
      const target = await confirmedWorkspaceTarget(input.switchWorkspace, workspaceId, paneId);
      if (!target) throw Error(input.failureMessage);
      if (version === generation && input.isAlive()) input.navigate(workspaceId, paneId);
    }).catch(error => { if (version === generation && input.isAlive()) input.onError(error); });
    return queue;
  };
}
