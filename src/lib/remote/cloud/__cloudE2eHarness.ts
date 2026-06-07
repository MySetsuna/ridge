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
// 前置：dev:cdp 以 RIDGE_CLOUD_BASE_DOMAIN=localhost:5050 启动（apiClient BASE_DOMAIN →
// 本地，scheme → http/ws）；ridge-cloud 跑在 :5050；DB 里有 premium 用户 + 该 device。

import { RidgeCloudHost } from './ridgeCloudProvider';
import { ControllerCloudProvider } from './controllerCloudProvider';
import { CloudHostBridge } from './cloudHostBridge';
import { createCloudWebrtcTransportWith } from '../../transport/remote/cloudWebrtcAdapter';
import { RpcClient } from '../../transport/remote/rpcClient';
import type { KeyBindingMode } from './keyBinding';
import { invoke } from '@tauri-apps/api/core';

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
  const push = (s: string) => log.push(`${log.length}:${s}`);

  // ── HOST（answerer）：createBridge 注入真 Tauri invoke ──────────────────────
  const host = new RidgeCloudHost(
    { deviceToken, username },
    {
      onHostState: (s) => push(`host:${s}`),
      onError: (m, c) => push(`host-err:${c ?? ''}:${m}`),
      // CloudHostBridge 直接满足 CloudHostBridgeLike（handleFrame/verifyPeerKey/reset）。
      // 无 keyBindingVerifier → 默认 relay-trust（与当前生产行为一致）。
      createBridge: (_cid, send) =>
        new CloudHostBridge({
          invoke: (method, params) => invoke(method, params ?? {}),
          sendFrame: send,
        }),
    },
  );

  // ── CONTROLLER（offerer）：adapter + L2 RpcClient（捕获 provider 以读绑定模式）──
  // 定值断言：createCloudWebrtcTransportWith 同步调用工厂，故 connect 前必已赋值。
  let controllerProvider!: ControllerCloudProvider;
  const adapter = createCloudWebrtcTransportWith(device, (cb) => {
    controllerProvider = new ControllerCloudProvider({ userToken, username }, cb);
    return controllerProvider;
  });
  const rpc = new RpcClient(adapter, { defaultTimeoutMs: timeoutMs });

  // B3 验证 seam：让 host 发错误信令公钥（仅本 dev harness 置位此 global，生产永不设）。
  const tamperGlobal = globalThis as { __RIDGE_DEBUG_TAMPER_E2EE_SIG?: boolean };

  let connected = false;
  try {
    if (opts.tamperBinding) tamperGlobal.__RIDGE_DEBUG_TAMPER_E2EE_SIG = true;
    await host.goOnline(device);
    push('host.goOnline returned');

    connected = await new Promise<boolean>((resolve) => {
      const to = setTimeout(() => {
        push('controller connect TIMEOUT');
        resolve(false);
      }, timeoutMs);
      const unsub = adapter.onStateChange((s) => {
        push(`ctrl:${s}`);
        if (s === 'connected') {
          clearTimeout(to);
          unsub();
          resolve(true);
        } else if (s === 'error') {
          clearTimeout(to);
          unsub();
          resolve(false);
        }
      });
      void adapter.connect();
    });

    const results: CloudE2eProbe[] = [];
    let capabilities: string[] | null = null;

    if (connected) {
      rpc.hello(); // D9 $/hello
      for (const offset of offsets) {
        try {
          const page = (await rpc.request('get_directory_children', {
            path,
            offset,
            limit,
          })) as { entries?: Array<{ name?: string }>; total?: number; has_more?: boolean };
          results.push({
            offset,
            ok: true,
            entries: page.entries?.length,
            total: page.total,
            hasMore: page.has_more,
            first: page.entries?.[0]?.name,
          });
        } catch (e) {
          results.push({ offset, ok: false, error: e instanceof Error ? e.message : String(e) });
        }
      }
      capabilities = rpc.protocol ? [...rpc.protocol.capabilities] : null;
    }

    let exploitResult: CloudE2eResult['exploitResult'] = null;
    if (connected && opts.exploit) {
      try {
        const r = await rpc.request(opts.exploit.method, opts.exploit.params ?? {});
        exploitResult = { method: opts.exploit.method, ok: true, sample: JSON.stringify(r).slice(0, 300) };
      } catch (e) {
        exploitResult = {
          method: opts.exploit.method,
          ok: false,
          error: e instanceof Error ? e.message : String(e),
        };
      }
    }

    const keyBindingMode: KeyBindingMode = controllerProvider.getKeyBindingMode();

    return { connected, results, capabilities, exploitResult, keyBindingMode, log };
  } finally {
    delete tamperGlobal.__RIDGE_DEBUG_TAMPER_E2EE_SIG;
    try {
      adapter.close();
      adapter.dispose();
    } catch {
      /* ignore */
    }
    try {
      host.goOffline();
    } catch {
      /* ignore */
    }
  }
}
