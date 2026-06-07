// src/lib/transport/remote/cloudWebrtcAdapter.ts
//
// L1 ChannelTransport adapter for the cloud-WebRTC leg (handoff plan §5.3 / §5.5,
// decision D5; contract §7). Wraps a `RemoteConnectionProvider` (the role-
// agnostic cloud transport primitive — `RidgeCloudProvider` and the future
// controller-side / Rust-migrated provider both satisfy it) so the desktop SPA's
// `bridge` + shared L2 `RpcClient` ride WebRTC unchanged, exactly as they ride
// LAN-WS via `lanWsAdapter.ts`.
//
// What this adapter does (and ONLY this — it is a "channel primitives over an
// already-authenticated, already-E2EE transport" wrapper, per plan §5.5):
//
//   • 1-byte mux DEMUX (inbound): each decrypted DataChannel plaintext frame is
//     tagged by its leading byte (see cloudMux.ts) →
//       0x10 PANE_RAW → strip header → onPaneBytes(paneId, bytes)
//       0x11 JSON      → JSON.parse → onControl(frame)   (JSON-RPC 2.0 envelope)
//   • 1-byte mux REMUX (outbound):
//       sendControl(json)        → 0x11 || utf8(JSON)
//       sendPaneBytes(paneId,b)  → 0x10 || paneIdLen || paneId || bytes
//   • State mapping: CloudConnectionState → TransportState, including the
//     reconnect edge L2 needs (so in-flight requests reject + the bridge
//     re-subscribes / re-handshakes — see rpcClient.handleStateChange).
//
// What it does NOT do (NOT in scope; owned elsewhere):
//   • E2EE (X25519 + ChaCha20-Poly1305) — happens INSIDE the provider; this
//     adapter only ever sees decrypted plaintext (provider.onFrame) and hands
//     plaintext down (provider.sendFrame). Plan §5.5: "各适配器自完成鉴权握手;
//     bridge 只要一个已鉴权的传输".
//   • JWT / device-pairing auth — owned by the provider + auth.ts.
//   • The host onFrame pipeline + Rust WebRTC migration — that is S4-host
//     runtime work; this is the pure-TS client leg.

import { demuxFrame, encodeControlFrame, encodeJsonFrame, encodePaneFrame } from './cloudMux';
import type {
  CloudConnectionCallbacks,
  CloudConnectionState,
  RemoteConnectionProvider,
} from '../../remote/cloud/connectionProvider';
import {
  type ChannelTransport,
  type ControlFrame,
  type ControlListener,
  type OutboundFrame,
  type PaneBytesListener,
  type StateListener,
  type TransportState,
  type Unsubscribe,
} from './types';

/**
 * Map the provider's connection state to the L1 transport state.
 *
 * The provider's `handshaking` (E2EE/D9 in progress) and `connecting` are both
 * "not yet ready for business frames" → surfaced as `connecting`. L2 only acts
 * on the `connected → (disconnected|error)` edge (reject in-flight) and the
 * `→ connected` edge (re-handshake + resync), so collapsing `handshaking` into
 * `connecting` keeps those edges correct while the E2EE/D9 handshake runs.
 */
function mapState(s: CloudConnectionState): TransportState {
  switch (s) {
    case 'connected':
      return 'connected';
    case 'connecting':
    case 'handshaking':
      return 'connecting';
    case 'error':
      return 'error';
    case 'disconnected':
    default:
      return 'disconnected';
  }
}

export class CloudWebrtcAdapter implements ChannelTransport {
  private readonly provider: RemoteConnectionProvider;
  private readonly deviceId: string;

  private controlListeners = new Set<ControlListener>();
  private paneListeners = new Set<PaneBytesListener>();
  private stateListeners = new Set<StateListener>();
  /**
   * Listeners for the 0x12 session-CONTROL channel (contract §4 TOTP handshake).
   * Kept separate from {@link controlListeners} (the 0x11 JSON-RPC business
   * channel) so the cloud-controller boot can gate readiness on `totp-result`
   * without entangling the L2 RPC client.
   */
  private sessionControlListeners = new Set<(frame: Record<string, unknown>) => void>();

  // Mirror of the provider state. The provider sets callbacks at construction;
  // the boot code hands it `adapter.callbacks` so the adapter is the single
  // owner of demux + state fan-out (and stays decoupled from the concrete
  // provider class).
  private lastState: TransportState;

  /**
   * @param provider an already-configured cloud provider. Its callbacks MUST be
   *        wired to {@link callbacks} (the factory below does this for the
   *        common `RidgeCloudProvider` path).
   * @param deviceId the device to connect to on {@link connect}.
   */
  constructor(provider: RemoteConnectionProvider, deviceId: string) {
    this.provider = provider;
    this.deviceId = deviceId;
    this.lastState = mapState(provider.getState());
  }

  /**
   * Callbacks to hand to the underlying provider's constructor. Centralising
   * them here means the adapter owns demux + state mapping regardless of which
   * provider implementation is injected. If the caller already passed other
   * callbacks (e.g. a UI `onError` toast), it can compose them around these.
   */
  get callbacks(): Required<Pick<CloudConnectionCallbacks, 'onState' | 'onFrame'>> &
    CloudConnectionCallbacks {
    return {
      onState: (s) => this.handleProviderState(s),
      onFrame: (plaintext) => this.handleInboundFrame(plaintext),
    };
  }

  // ── L1: control channel ─────────────────────────────────────────────────────
  sendControl(frame: OutboundFrame): void {
    this.provider.sendFrame(encodeJsonFrame(frame));
  }

  onControl(cb: ControlListener): Unsubscribe {
    this.controlListeners.add(cb);
    return () => this.controlListeners.delete(cb);
  }

  // ── §4 session-CONTROL channel (0x12): TOTP handshake ────────────────────────
  /** Send a session-control frame (e.g. `{ t: 'totp-verify', code }`) on 0x12. */
  sendSessionControl(frame: Record<string, unknown>): void {
    this.provider.sendFrame(encodeControlFrame(frame));
  }

  /** Subscribe to inbound 0x12 session-control frames (e.g. `{ t: 'totp-result', ok }`). */
  onSessionControl(cb: (frame: Record<string, unknown>) => void): Unsubscribe {
    this.sessionControlListeners.add(cb);
    return () => this.sessionControlListeners.delete(cb);
  }

  // ── L1: pane bytes ──────────────────────────────────────────────────────────
  sendPaneBytes(paneId: string, bytes: Uint8Array): void {
    this.provider.sendFrame(encodePaneFrame(paneId, bytes));
  }

  onPaneBytes(cb: PaneBytesListener): Unsubscribe {
    this.paneListeners.add(cb);
    return () => this.paneListeners.delete(cb);
  }

  // ── L1: lifecycle ───────────────────────────────────────────────────────────
  connect(): Promise<void> {
    // The provider owns the signaling/ICE/DTLS + E2EE + (D9) auth handshake.
    return this.provider.connect(this.deviceId);
  }

  close(): void {
    this.provider.disconnect();
  }

  state(): TransportState {
    return mapState(this.provider.getState());
  }

  onStateChange(cb: StateListener): Unsubscribe {
    this.stateListeners.add(cb);
    return () => this.stateListeners.delete(cb);
  }

  // ── inbound demux ────────────────────────────────────────────────────────────
  /** One decrypted DataChannel plaintext frame → control frame or pane bytes. */
  private handleInboundFrame(plaintext: Uint8Array): void {
    let result;
    try {
      result = demuxFrame(plaintext);
    } catch (e) {
      // Malformed JSON / structurally bad frame: drop it (do not tear the
      // connection down — matches the provider's per-frame reject stance).
      console.error('[cloudWebrtcAdapter] failed to demux inbound frame', e);
      return;
    }

    switch (result.kind) {
      case 'json':
        // The 0x11 channel is the JSON-RPC 2.0 control/invoke envelope. Only a
        // JSON object is a valid control frame; ignore non-object payloads.
        if (result.json !== null && typeof result.json === 'object') {
          this.emitControl(result.json as ControlFrame);
        }
        return;
      case 'control':
        // The 0x12 channel is the §4 session-control envelope (TOTP handshake).
        if (result.json !== null && typeof result.json === 'object') {
          this.emitSessionControl(result.json as Record<string, unknown>);
        }
        return;
      case 'pane':
        this.emitPaneBytes(result.paneId, result.bytes);
        return;
      case 'unknown':
        // Forward-compat: a future channel tag the client doesn't know → ignore.
        return;
    }
  }

  private handleProviderState(s: CloudConnectionState): void {
    const mapped = mapState(s);
    if (mapped === this.lastState) return; // collapse handshaking→connecting churn
    this.lastState = mapped;
    for (const cb of this.stateListeners) {
      try {
        cb(mapped);
      } catch (e) {
        console.error('[cloudWebrtcAdapter] state listener threw', e);
      }
    }
  }

  private emitControl(frame: ControlFrame): void {
    for (const cb of this.controlListeners) {
      try {
        cb(frame);
      } catch (e) {
        console.error('[cloudWebrtcAdapter] control listener threw', e);
      }
    }
  }

  private emitPaneBytes(paneId: string, bytes: Uint8Array): void {
    for (const cb of this.paneListeners) {
      try {
        cb(paneId, bytes);
      } catch (e) {
        console.error('[cloudWebrtcAdapter] pane-bytes listener threw', e);
      }
    }
  }

  private emitSessionControl(frame: Record<string, unknown>): void {
    for (const cb of this.sessionControlListeners) {
      try {
        cb(frame);
      } catch (e) {
        console.error('[cloudWebrtcAdapter] session-control listener threw', e);
      }
    }
  }

  /** Detach all listeners (does not disconnect the provider). */
  dispose(): void {
    this.controlListeners.clear();
    this.paneListeners.clear();
    this.stateListeners.clear();
    this.sessionControlListeners.clear();
  }
}

/**
 * Build an adapter around an existing, already-configured provider whose
 * callbacks are (or will be) wired to `adapter.callbacks`. Use this when the
 * caller constructs the provider itself (e.g. to compose extra UI callbacks).
 */
export function createCloudWebrtcTransport(
  provider: RemoteConnectionProvider,
  deviceId: string,
): CloudWebrtcAdapter {
  return new CloudWebrtcAdapter(provider, deviceId);
}

/**
 * Build an adapter AND its provider in one call. `makeProvider` receives the
 * adapter's demux/state callbacks and must return a provider wired to them
 * (the common `RidgeCloudProvider` path):
 *
 *   bridge.attach(
 *     createCloudWebrtcTransportWith(deviceId, (cb) =>
 *       new RidgeCloudProvider(cfg, cb)),
 *   );
 *
 * This keeps the transport layer free of any concrete-provider import while
 * guaranteeing the provider's frames/state reach the adapter.
 */
export function createCloudWebrtcTransportWith(
  deviceId: string,
  makeProvider: (callbacks: CloudConnectionCallbacks) => RemoteConnectionProvider,
): CloudWebrtcAdapter {
  // Defer wiring: the adapter exposes the callbacks, the factory builds the
  // provider around them, then we hand the provider to the adapter.
  let adapter: CloudWebrtcAdapter | null = null;
  const callbacks: CloudConnectionCallbacks = {
    onState: (s) => adapter?.callbacks.onState(s),
    onFrame: (b) => adapter?.callbacks.onFrame(b),
  };
  const provider = makeProvider(callbacks);
  adapter = new CloudWebrtcAdapter(provider, deviceId);
  return adapter;
}
