/**
 * cloudHostStore.ts — 公网远控 host 的**模块级单例**。
 *
 * 背景（修复）：`RidgeCloudHost` 原先是 `RemotePanel.svelte` 的组件局部变量，且
 * 组件 `onMount` 清理里调了 `host.goOffline()`。而折叠侧边栏会用 `{#if !sidebarCollapsed}`
 * 把整个侧栏（连同 RemotePanel）卸载 → 触发清理 → 公网连接被掐断。
 *
 * 把 host 的所有权与生命周期提升到这个模块单例后：连接状态只由用户显式的
 * 上线/下线驱动，与任何面板的挂载/卸载、Tab 切换、侧栏折叠全部解耦。RemotePanel
 * 只「订阅」这里的 store + 调用这里的动作；逻辑（buildHost / E2EE 握手桥接 / 设备
 * 签名 / TOTP 校验注入）原样搬过来，行为不变。
 */
import { writable, get } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { tr } from '$lib/i18n';
import { cloudHostOnline } from '$lib/stores/remoteStatus';
import * as cloudAuth from '@ridge/remote/shared/cloud/auth';
import * as cloudApi from '@ridge/remote/shared/cloud/apiClient';
import { RidgeCloudHost, type CloudControllerSession, type HostSignalState } from '@ridge/remote/shared/cloud/ridgeCloudProvider';
import { CloudHostBridge } from '@ridge/remote/shared/cloud/cloudHostBridge';
import { makeCloudHostPaneSource } from '@ridge/remote/shared/cloud/cloudHostPaneSource';
import {
  collectPaneIds,
  filterWorkspaceResult,
  planWorkspaceInvoke,
  type WorkspaceAccess,
  type WorkspaceScopeAssertion,
} from '@ridge/remote/shared/cloud/workspaceScope';

// ── 公开响应式状态（组件用 `$hostState` 等订阅）─────────────────────────────
export const hostState = writable<HostSignalState>('offline');
export const cloudSessions = writable<CloudControllerSession[]>([]);
/** host 路径产生的错误文案（与面板自身的 LAN/账户错误分开）。 */
export const hostError = writable('');

// ── 模块级单例（不随组件卸载销毁）─────────────────────────────────────────
let host: RidgeCloudHost | null = null;
// 本机 Ed25519 设备身份公钥（取一次缓存），握手发 0x02 设备签名帧。
let deviceIdentityPub: Uint8Array | null = null;

async function refreshWorkspaceAccess(access: WorkspaceAccess): Promise<void> {
  const layout = await invoke<unknown>('get_pane_layout_for', {
    workspaceId: access.workspaceId,
  });
  const paneIds = collectPaneIds(layout);
  const roots = new Set<string>();
  for (const paneId of paneIds) {
    const cwd = await invoke<string | null>('get_pane_cwd', {
      workspaceId: access.workspaceId,
      paneId,
    });
    if (!cwd) continue;
    roots.add(cwd);
    const repoRoot = await invoke<string | null>('find_git_repo_root', { path: cwd });
    if (repoRoot) roots.add(repoRoot);
  }
  access.paneIds = paneIds;
  access.roots = [...roots];
}

async function invokeWorkspaceScoped(
  access: WorkspaceAccess,
  method: string,
  params?: unknown,
): Promise<unknown> {
  await refreshWorkspaceAccess(access);
  const plan = planWorkspaceInvoke(method, params, access);
  if (plan.kind === 'deny') {
    throw {
      code: -32003,
      message: `workspace share denied: ${plan.reason}`,
      data: { kind: 'scope_denied', workspaceId: access.workspaceId },
    };
  }
  if (plan.kind === 'result') return plan.value;
  const result = await invoke<unknown>(plan.method, plan.params);
  if (
    plan.method === 'split_pane' ||
    plan.method === 'dock_pane' ||
    plan.method === 'close_pane' ||
    plan.method === 'create_pane'
  ) {
    await refreshWorkspaceAccess(access);
  }
  return filterWorkspaceResult(method, result, access.workspaceId);
}

// 跨 agent 命令：通知 Rust 侧云端远控活跃状态（契约 §8.1）。容错。
async function notifyCloudActive(active: boolean): Promise<void> {
  try {
    await invoke('set_cloud_remote_active', { active });
  } catch {
    /* 容错 */
  }
}

/** 构造 host 管理器：每个 controller 一个独立 CloudHostBridge（pane 输出各自订阅）。 */
function buildHost(): RidgeCloudHost | null {
  const s = cloudAuth.snapshot();
  if (!s.deviceToken || !s.deviceName || !s.user?.username) return null;
  return new RidgeCloudHost(
    {
      deviceToken: s.deviceToken,
      username: s.user.username,
      // 零信任 #2（概念 4-桌面）：host 握手发 0x02 设备签名帧。signContext = 对 id-bind
      // context 做 Ed25519 签名（私钥在 Rust/DPAPI，relay 无法伪造）；identityPub = 本机
      // 设备身份公钥（启动取一次缓存）。两者配对：俱在 → 0x02；缺一 → 回落 0x01（向后兼容）。
      signContext: (context: Uint8Array) =>
        invoke<number[]>('sign_device_identity', { context: Array.from(context) }).then((a) =>
          Uint8Array.from(a),
        ),
      identityPub: deviceIdentityPub ?? undefined,
    },
    {
      verifyWorkspaceScope: async (token): Promise<WorkspaceScopeAssertion | null> => {
        const current = cloudAuth.snapshot();
        if (!current.deviceToken) return null;
        return cloudApi.verifyWorkspaceShareAccess(current.deviceToken, token);
      },
      onHostState: (st) => {
        hostState.set(st);
        if (st === 'error') hostError.set(tr('cloud.hostError'));
        if (st === 'online' || st === 'connecting') hostError.set('');
        // Surface "public remote is serving" to the whole app so per-pane
        // refresh buttons (RidgePane) appear while a cloud viewer can share
        // the PTY — the LAN-only `remoteRunning` store stays false here.
        cloudHostOnline.set(st === 'online');
      },
      onSessions: (list) => {
        cloudSessions.set(list);
      },
      onError: (msg) => {
        hostError.set(msg);
      },
      // host=Tauri 桌面 app：注入真实 invoke + pane 源 + 本机 TOTP 校验（契约 §0/§4/§5.1）。
      createBridge: (_cid, send, bindTranscript, workspaceScope) => {
        const access: WorkspaceAccess | null = workspaceScope
          ? { ...workspaceScope, paneIds: new Set(), roots: [] }
          : null;
        const paneSource = makeCloudHostPaneSource({ invoke, listen });
        return new CloudHostBridge({
          invoke: (method, params) =>
            access ? invokeWorkspaceScoped(access, method, params) : invoke(method, params),
          sendFrame: send,
          preauthorized: !!access,
          // B2（D-GM-11）：用 subscribe_pane_raw 专用 raw fan-out（RemotePtyEvent::
          // RawBytes → Tauri event pane-raw-{pane}）。
          paneOutputSource: access
            ? (paneId, workspaceId, onOutput) =>
                access.paneIds.has(paneId)
                  ? paneSource(paneId, workspaceId, onOutput)
                  : () => {}
            : paneSource,
          // 明文 totp-verify（旧 controller / host 回落 0x01 时）。
          totpVerifier: access
            ? undefined
            : (code) => invoke<boolean>('verify_remote_totp', { code }),
          // 零信任 #1（概念 5）：host 发 0x02 → bindTranscript 非空时启用 totp-bind
          // 信道绑定校验（HMAC tag，明文码不上线）。
          totpBindVerifier: !access && bindTranscript
            ? (tag) =>
                invoke<boolean>('verify_remote_totp_bind', {
                  transcript: Array.from(bindTranscript),
                  tag: Array.from(tag),
                })
            : undefined,
          // §7.4 trusted-controller grant：注入信道绑定 transcript 供 Ed25519 proof 验证。
          bindTranscript,
          // iter-60 G9：把 host 的 pane 元信息/布局事件推给云控制端（此前 cloud 腿
          // 零事件推送，手机头部标题/CWD 与 Pane 弹层只能靠轮询/重连刷新）。
          hostEventSource: (emit) => {
            const names = ['pane-meta-changed', 'pane-tree-changed'] as const;
            const unsubs: Array<() => void> = [];
            for (const name of names) {
              listen(name, (e) => {
                if (!access) {
                  emit(name, e.payload);
                  return;
                }
                const workspaceId =
                  e.payload && typeof e.payload === 'object'
                    ? (e.payload as Record<string, unknown>).workspaceId
                    : undefined;
                if (workspaceId === access.workspaceId) emit(name, e.payload);
              })
                .then((un) => unsubs.push(un))
                .catch((error) => console.warn('[cloud-host] event listener failed', error));
            }
            return () => { for (const un of unsubs) un(); };
          },
        });
      },
    },
  );
}

/** 上线公网远控。幂等：已构造的 host 复用。 */
export async function goOnline(): Promise<void> {
  hostError.set('');
  const s = cloudAuth.snapshot();
  if (isTauri()) {
    if (!s.deviceToken || !s.deviceName || !s.user?.username) {
      hostError.set(tr('cloud.errDeviceNotActivated'));
      return;
    }
    // Native desktop owns no WebRTC transport in its WebView. Hand
    // credentials to the detached daemon so a force-killed Tauri shell
    // cannot disconnect Remote. Browser/PWA uses the fallback below.
    hostState.set('connecting');
    cloudHostOnline.set(false);
    try {
      await invoke('sync_cloud_remote_credentials', {
        deviceToken: s.deviceToken,
        deviceName: s.deviceName,
        username: s.user?.username ?? null,
      });
      await invoke('ensure_cloud_remote_host');
      hostState.set('online');
      cloudHostOnline.set(true);
    } catch (e) {
      hostState.set('error');
      cloudHostOnline.set(false);
      hostError.set(e instanceof Error ? e.message : tr('cloud.errConnectFailed'));
    }
    return;
  }
  if (!s.deviceToken || !s.deviceName || !s.user?.username) {
    hostError.set(tr('cloud.errDeviceNotActivated'));
    return;
  }
  // 零信任 #2（概念 4-桌面）：取一次本机设备身份公钥缓存，供 host 握手发 0x02。
  // 取不到（旧设备/无密钥）→ 留 null，host 自动回落 0x01（不阻断上线）。
  if (!deviceIdentityPub) {
    try {
      deviceIdentityPub = Uint8Array.from(await invoke<number[]>('get_device_identity_pub'));
    } catch {
      deviceIdentityPub = null;
    }
  }
  host ??= buildHost();
  if (!host) {
    hostError.set(tr('cloud.errDeviceNotActivated'));
    return;
  }
  try {
    await host.goOnline(s.deviceName);
    await notifyCloudActive(true);
  } catch (e) {
    hostError.set(e instanceof Error ? e.message : tr('cloud.errConnectFailed'));
  }
}

/** 下线公网远控。只有用户显式调用（或 kick/blacklist 不涉及）才会断开。 */
export async function goOffline(): Promise<void> {
  if (isTauri()) {
    try {
      await invoke('disable_cloud_remote_host');
    } catch (e) {
      hostError.set(e instanceof Error ? e.message : tr('cloud.errConnectFailed'));
    }
    hostState.set('offline');
    cloudHostOnline.set(false);
    return;
  }
  host?.goOffline();
  cloudHostOnline.set(false);
  await notifyCloudActive(false);
}

/** 主动断开某 controller。 */
export function kickController(cid: string): void {
  host?.kick(cid);
}

/** 拉黑某 controller。 */
export function blacklistController(cid: string): void {
  host?.blacklist(cid);
}

/** 当前是否在线（命令式读取，供非响应式场景）。 */
export function isHostOnline(): boolean {
  return get(hostState) === 'online';
}
