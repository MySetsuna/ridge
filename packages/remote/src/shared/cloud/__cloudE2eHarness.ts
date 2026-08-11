// DEV-ONLY 诊断 harness — 不被任何生产代码 import，故生产构建会 tree-shake 掉。
//
// 目的：在 dev:cdp 的 Tauri webview（真 invoke）里，于**同一 JS realm** 同时实例化
// cloud host provider + cloud controller provider，经本地 ridge-cloud relay 互连，
// 真实跑通 WebRTC + E2EE + 1字节 mux + JSON-RPC dispatch → host CloudHostBridge →
// 真 Tauri `invoke('get_directory_children')`。用来复现/确认 B1（dir-children 经云
// 是否分页正确），并证明整条云链路端到端可用。
//
// 用法（经 CDP evaluate_script）：
//   const m = await import('/src/lib/remote/cloud/__cloudE2eHarness.ts');
//   const r = await m.runCloudDirChildrenE2E({ deviceToken, userToken, username:'alice',
//                                              device:'mylaptop', path:'C:\\code\\wind' });
//   // r.results = 各 offset 的分页结果；r.log = 状态轨迹
//
// 前置：dev:cdp 以 RIDGE_CLOUD_BASE_DOMAIN=localhost:5050 RIDGE_CLOUD_DEV_PLAINTEXT=1
// 启动（apiClient BASE_DOMAIN →本地，scheme → http/ws）；ridge-cloud 跑在 :5050；DB 里有
// premium 用户 + 该 device。

import { RidgeCloudHost } from './ridgeCloudProvider';
import { ControllerCloudProvider } from './controllerCloudProvider';
import { CloudHostBridge } from './cloudHostBridge';
import {
  CLIENT_CAPABILITIES,
  CLIENT_PROTOCOL_VERSION,
  HELLO_METHOD,
  RpcClient,
  createCloudWebrtcTransportWith,
} from '@ridge/remote';
import type { KeyBindingMode } from './keyBinding';
import { makeCloudHostPaneSource } from './cloudHostPaneSource';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface CloudE2eOptions {
  /** device JWT（scope=device）。 */
  deviceToken: string;
  /** user JWT（scope=user，premium）。 */
  userToken: string;
  /** 账户用户名（host label 段；两端必须同账户）。 */
  username: string;
  /** 目标 host 的 device_name（房间 label 的 device 段）。 */
  device: string;
  /** 要列目录的主机绝对路径（host 端真实路径，如 C:\\code\\wind）。 */
  path: string;
  /** 要探的 offset 列表（默认 0/3/6 复刻 B1 探针）。 */
  offsets?: number[];
  /** 每页条数（默认 3）。 */
  limit?: number;
  /** 连接 + 每次 invoke 超时（ms）。 */
  timeoutMs?: number;
  /**
   * 可选：连上后额外探一个任意命令（用于验证审计 #1：云桥是否对 controller
   * 任意 method 无白名单直送 invoke）。如 { method: 'get_remote_info' }。
   */
  exploit?: { method: string; params?: Record<string, unknown> };
  /**
   * B3 验证：置位时让 **host** 经信令旁路发送**错误**的临时公钥（模拟 relay-MITM 在
   * E2EE 腿调包）。预期 controller 比对失败 → 判 MITM 拒绝 → connected=false。
   */
  tamperBinding?: boolean;
  /**
   * B2 验证：连上后订阅一个 pane 的裸字节流（`subscribe-pane`），可选先 `write_to_pty`
   * 触发输出，收集经云回推的 pane 帧。证明终端经云端到端可用。
   */
  paneStream?: { paneId: string; write?: string; waitMs?: number };
}

export interface CloudE2eProbe {
  offset: number;
  ok: boolean;
  entries?: number;
  total?: number;
  hasMore?: boolean;
  first?: string;
  error?: string;
}

export interface CloudE2eResult {
  /** 连接是否成功建立到 connected。 */
  connected: boolean;
  /** 各 offset 的 get_directory_children 结果。 */
  results: CloudE2eProbe[];
  /** D9 协商出的能力集（证明 $/hello 往返）。 */
  capabilities: string[] | null;
  /** host/controller 状态轨迹 + 错误，便于诊断。 */
  log: string[];
  /** 可选 exploit 探针结果（审计 #1 验证）。 */
  exploitResult?: { method: string; ok: boolean; sample?: string; error?: string } | null;
  /** B3：controller 端最终绑定模式（enforced=信令公钥已比对一致；relay-trust=回落）。 */
  keyBindingMode?: KeyBindingMode | null;
  /** B2：pane 裸字节流验证结果（经云收到的帧数/字节数/样本）。 */
  paneStream?: { paneId: string; frames: number; bytes: number; sample: string } | null;
}

/**
 * 单 realm 跑通 cloud host↔controller，经云调用 get_directory_children 多个 offset。
 * 永不抛错——失败信息收进返回值的 log/results，便于 CDP 取回。
 */
export async function runCloudDirChildrenE2E(opts: CloudE2eOptions): Promise<CloudE2eResult> {
  const {
    deviceToken,
    userToken,
    username,
    device,
    path,
    offsets = [0, 3, 6],
    limit = 3,
    timeoutMs = 20_000,
  } = opts;
  const log: string[] = [];
  const push = (message: string) => log.push(`${log.length}:${message}`);

  const host = new RidgeCloudHost(
    { deviceToken, username },
    {
      onHostState: (state) => push(`host:${state}`),
      onError: (message, code) => push(`host-err:${code ?? ''}:${message}`),
      createBridge: (_cid, send) => new CloudHostBridge({
        invoke: (method, params) => invoke(method, params ?? {}),
        sendFrame: send,
        paneOutputSource: makeCloudHostPaneSource({
          invoke: (command, args) => invoke(command, args ?? {}),
          listen: listen as never,
        }),
      }),
    },
  );

  let controllerProvider!: ControllerCloudProvider;
  const adapter = createCloudWebrtcTransportWith(device, (callback) => {
    controllerProvider = new ControllerCloudProvider({ userToken, username }, callback);
    return controllerProvider;
  });
  const offControllerError = adapter.onError((message, code) =>
    push(`ctrl-err:${code ?? ''}:${message}`),
  );
  const rpc = new RpcClient(adapter, { defaultTimeoutMs: timeoutMs });
  const tamperGlobal = globalThis as { __RIDGE_DEBUG_TAMPER_E2EE_SIG?: boolean };

  const connect = (): Promise<boolean> => new Promise((resolve) => {
    const timer = setTimeout(() => {
      push('controller connect TIMEOUT');
      resolve(false);
    }, timeoutMs);
    const unsubscribe = adapter.onStateChange((state) => {
      push(`ctrl:${state}`);
      if (state !== 'connected' && state !== 'error') return;
      clearTimeout(timer);
      unsubscribe();
      resolve(state === 'connected');
    });
    void adapter.connect();
  });

  const negotiate = (): Promise<string[] | null> => new Promise((resolve) => {
    let unsubscribe = () => {};
    const timer = setTimeout(() => {
      unsubscribe();
      resolve(null);
    }, timeoutMs);
    unsubscribe = rpc.onNegotiated((protocol) => {
      clearTimeout(timer);
      unsubscribe();
      resolve([...protocol.capabilities]);
    });
    rpc.notify(HELLO_METHOD, {
      protocolVersion: CLIENT_PROTOCOL_VERSION,
      capabilities: [...CLIENT_CAPABILITIES],
    });
  });

  const probeDirectories = async (): Promise<CloudE2eProbe[]> => {
    const results: CloudE2eProbe[] = [];
    for (const offset of offsets) {
      try {
        const page = await rpc.request('get_directory_children', { path, offset, limit }) as {
          entries?: Array<{ name?: string }>;
          total?: number;
          has_more?: boolean;
        };
        results.push({
          offset,
          ok: true,
          entries: page.entries?.length,
          total: page.total,
          hasMore: page.has_more,
          first: page.entries?.[0]?.name,
        });
      } catch (error) {
        results.push({ offset, ok: false, error: error instanceof Error ? error.message : String(error) });
      }
    }
    return results;
  };

  const probeExploit = async (): Promise<CloudE2eResult['exploitResult']> => {
    if (!opts.exploit) return null;
    try {
      const result = await rpc.request(opts.exploit.method, opts.exploit.params ?? {});
      return { method: opts.exploit.method, ok: true, sample: JSON.stringify(result).slice(0, 300) };
    } catch (error) {
      return {
        method: opts.exploit.method,
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  };

  const probePane = async (): Promise<CloudE2eResult['paneStream']> => {
    if (!opts.paneStream) return null;
    const { paneId, write, waitMs = 1500 } = opts.paneStream;
    let frames = 0;
    let bytes = 0;
    let sample = '';
    const decoder = new TextDecoder();
    const offPane = adapter.onPaneBytes((id, chunk) => {
      if (id !== paneId) return;
      frames += 1;
      bytes += chunk.length;
      if (sample.length < 160) sample += decoder.decode(chunk, { stream: true });
    });
    rpc.notify('subscribe-pane', { paneId });
    if (write) {
      try {
        await rpc.request('write_to_pty', { paneId, data: write });
      } catch (error) {
        push(`write_to_pty err:${error instanceof Error ? error.message : String(error)}`);
      }
    }
    await new Promise<void>((resolve) => setTimeout(resolve, waitMs));
    offPane();
    return { paneId, frames, bytes, sample: sample.slice(0, 160) };
  };

  let connected = false;
  try {
    if (opts.tamperBinding) tamperGlobal.__RIDGE_DEBUG_TAMPER_E2EE_SIG = true;
    await host.goOnline(device);
    await invoke('set_cloud_remote_active', { active: true }).catch(() => {});
    push('host.goOnline returned');
    connected = await connect();

    let capabilities: string[] | null = null;
    let results: CloudE2eProbe[] = [];
    if (connected) {
      capabilities = await negotiate();
      results = await probeDirectories();
    }
    const exploitResult = connected ? await probeExploit() : null;
    const keyBindingMode: KeyBindingMode = controllerProvider.getKeyBindingMode();
    const paneStream = connected ? await probePane() : null;
    return { connected, results, capabilities, exploitResult, keyBindingMode, paneStream, log };
  } finally {
    delete tamperGlobal.__RIDGE_DEBUG_TAMPER_E2EE_SIG;
    try {
      adapter.close();
      adapter.dispose();
    } catch {
      // Cleanup is best effort; the test result already contains the failure.
    }
    offControllerError();
    try {
      host.goOffline();
    } catch {
      // Cleanup is best effort.
    }
    await invoke('set_cloud_remote_active', { active: false }).catch(() => {});
  }
}
