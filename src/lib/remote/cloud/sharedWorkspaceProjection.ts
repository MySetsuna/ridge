import { writable } from 'svelte/store';
import { getWorkspaceShareToken } from '@ridge/remote/shared/cloud/apiClient';
import { snapshot as authSnapshot } from '@ridge/remote/shared/cloud/auth';
import type { PaneInfo } from '@ridge/remote';
import type { DataProvider } from '$lib/transport';
import { TauriDataProvider } from '$lib/transport/tauri';
import { TauriBridge } from '$lib/transport/tauriShim/bridge';
import { startCloudControllerBoot } from './cloudControllerBoot';
import { CloudRemoteConnection } from '../../../remote/lib/cloudRemote';

export interface SharedWorkspaceProjection {
  grantId: string;
  workspaceId: string;
  name: string;
  ownerUsername: string;
  deviceName: string;
  panes: PaneInfo[];
  link: CloudRemoteConnection;
  dataProvider: DataProvider;
}

export const activeSharedWorkspaceProjection = writable<SharedWorkspaceProjection | null>(null);

let current: SharedWorkspaceProjection | null = null;
let stopCurrentPaneSync = () => {};

function waitForShareAuthorization(
  link: ReturnType<typeof startCloudControllerBoot>['adapter'],
  timeoutMs = 20_000,
): Promise<void> {
  if (link.state() === 'connected' && link.authState() === 'authorized') {
    return Promise.resolve();
  }
  return new Promise((resolve, reject) => {
    let done = false;
    let timer: ReturnType<typeof setTimeout>;
    let stopState = () => {};
    let stopAuth = () => {};
    const finish = (error?: Error) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      stopState();
      stopAuth();
      error ? reject(error) : resolve();
    };
    const check = () => {
      if (link.state() === 'connected' && link.authState() === 'authorized') finish();
      else if (link.state() === 'error' || link.authState() === 'denied') {
        finish(new Error('共享工作区授权失败'));
      }
    };
    stopState = link.onStateChange(check);
    stopAuth = link.onAuthChange(check);
    timer = setTimeout(() => finish(new Error('共享工作区连接超时')), timeoutMs);
    check();
  });
}

export function assertShareTokenScope(
  input: { grantId: string; workspaceId: string; deviceName: string },
  scoped: { grantId: string; workspaceId: string; deviceName: string; delegable: boolean },
): void {
  if (
    scoped.grantId !== input.grantId
    || scoped.workspaceId !== input.workspaceId
    || scoped.deviceName !== input.deviceName
    || scoped.delegable !== false
  ) {
    throw new Error('共享工作区授权范围不匹配');
  }
}

export async function openSharedWorkspaceProjection(input: {
  grantId: string;
  workspaceId: string;
  name: string;
  ownerUsername: string;
  deviceName: string;
}): Promise<void> {
  const userToken = authSnapshot().userToken;
  if (!userToken) throw new Error('请先登录 Ridge Cloud');
  closeSharedWorkspaceProjection();

  const scoped = await getWorkspaceShareToken(userToken, input.grantId);
  assertShareTokenScope(input, scoped);

  const targetBridge = new TauriBridge();
  let connection: CloudRemoteConnection | null = null;
  const handle = startCloudControllerBoot(
    {
      userToken: scoped.token,
      fixedToken: true,
      hostDevice: scoped.deviceName,
      username: input.ownerUsername,
      onState: (state) => connection?.notifyState(state),
      onError: (message, code) => connection?.notifyError(message, code),
    },
    {
      isolated: true,
      targetBridge,
      useGlobalWorkspace: false,
      installGlobalTransport: false,
    },
  );

  try {
    await waitForShareAuthorization(handle.adapter);
    connection = new CloudRemoteConnection(handle, targetBridge, { fixedAuthorized: true });
    connection.notifyState('connected');
    await connection.init();
    const { workspaces } = await connection.listWorkspaces();
    if (workspaces.length !== 1 || workspaces[0].id !== scoped.workspaceId) {
      throw new Error('Host 返回了授权范围外的工作区');
    }
    const panes = await connection.listWorkspacePanes(scoped.workspaceId);
    const projection: SharedWorkspaceProjection = {
      ...input,
      name: workspaces[0].name || input.name,
      panes,
      link: connection,
      dataProvider: new TauriDataProvider(
        (method, args) => targetBridge.invoke(method, args),
      ),
    };
    current = projection;
    activeSharedWorkspaceProjection.set(projection);
    const updatePanes = (nextPanes: PaneInfo[]) => {
      if (current?.link !== connection) return;
      current = { ...current, panes: nextPanes };
      activeSharedWorkspaceProjection.set(current);
    };
    const stopMessages = connection.onMessage((message) => {
      if (message.type === 'panes') updatePanes(message.panes);
    });
    const stopMetadata = connection.onMetadata((paneId, title, cwd) => {
      if (!current || current.link !== connection) return;
      updatePanes(current.panes.map((pane) => pane.id === paneId
        ? { ...pane, title: title ?? undefined, cwd: cwd ?? undefined }
        : pane));
    });
    stopCurrentPaneSync = () => {
      stopMessages();
      stopMetadata();
    };
  } catch (error) {
    handle.disconnect();
    throw error;
  }
}

export function currentSharedWorkspaceProjection(): SharedWorkspaceProjection | null {
  return current;
}

export function closeSharedWorkspaceProjection(): void {
  stopCurrentPaneSync();
  stopCurrentPaneSync = () => {};
  current?.link.disconnect();
  current = null;
  activeSharedWorkspaceProjection.set(null);
}
