// src/lib/transport/remote/cloudMux.ts
//
// 1-byte channel-prefix mux codec for the cloud-WebRTC leg (handoff plan §5.3,
// contract §7). The WebRTC DataChannel carries a SINGLE byte stream of E2EE
// plaintext frames; a leading byte tags each frame's logical channel (sharing
// the convention `packages/ridge-cli/src/protocol.rs` already speaks):
//
//   0x10  PANE_RAW  — raw PTY bytes for one pane (high-frequency, one-way).
//   0x11  JSON      — UTF-8 JSON: control / event / invoke (JSON-RPC 2.0).
//
// This module is pure (no I/O, no DataChannel, no E2EE) so it can be unit-tested
// in isolation and reused by both the send and receive paths of the adapter.
//
// ── PANE_RAW paneId carrying (adapter-owned convention) ──────────────────────
// Contract §7 describes 0x10 as "paneId prefix + raw bytes", but ridge-cli's
// current `protocol.rs` 0x10 carries ONLY raw bytes (it is single-pane today,
// no paneId). The L1 `ChannelTransport` surface, however, is multi-pane
// (`sendPaneBytes(paneId, …)` / `onPaneBytes(paneId, …)`), so a paneId MUST be
// carried on the wire. We define a self-describing, forward-compatible framing
// that this adapter owns on BOTH directions:
//
//   0x10 || paneIdLen(1 byte, u8) || paneId(UTF-8, ≤255 bytes) || rawBytes…
//
// A 1-byte length prefix keeps the header tiny and is unambiguous for the pane
// ids this codebase uses (short opaque strings). The matching host-side encoder
// (the Rust desktop/headless host, S4-host) MUST align to this exact layout —
// see the report's "S4-host follow-ups" section. (`0x11` JSON is unchanged from
// ridge-cli, so the control channel is already byte-compatible.)

/** Logical channel tags (contract §7 / ridge-cli `protocol.rs` `channel`). */
export const CHANNEL = {
  /** Raw PTY bytes for a pane. */
  PANE_RAW: 0x10,
  /** UTF-8 JSON (control / event / invoke — the JSON-RPC 2.0 business envelope). */
  JSON: 0x11,
  /**
   * UTF-8 JSON session-CONTROL frames (contract §4): distinct from the 0x11
   * JSON-RPC business channel so the host can gate business frames while still
   * processing the TOTP handshake on a separate, always-open channel. Carries
   * `{ t: 'totp-verify', code }` (controller→host) and `{ t: 'totp-result', ok }`
   * (host→controller). Forward-compatible: more `t`-tagged control messages can
   * ride this channel later without touching the RPC/pane framing.
   */
  CONTROL: 0x12,
} as const;

/** Max bytes a paneId may occupy on the wire (1-byte length prefix). */
export const MAX_PANE_ID_BYTES = 255;

// SECURITY (audit #4): cap a decrypted plaintext frame's size BEFORE it is
// JSON-parsed / TextDecoder-decoded. A connected peer can otherwise send an
// arbitrarily large frame and OOM / stall the UI thread. Two caps because the
// channels differ: JSON/CONTROL envelopes are small (control / invoke), while
// PANE_RAW carries bursty PTY output that can legitimately be larger.
/** Max bytes for a JSON (0x11) / CONTROL (0x12) frame — control envelopes are small. */
export const MAX_JSON_FRAME_BYTES = 4 * 1024 * 1024; // 4 MiB
/** Max bytes for a PANE_RAW (0x10) frame — PTY bursts may be larger than control. */
export const MAX_PANE_FRAME_BYTES = 16 * 1024 * 1024; // 16 MiB

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

/** A demuxed inbound frame: a JSON-RPC control frame, a session-control frame, or pane bytes. */
export type DemuxResult =
  | { kind: 'json'; json: unknown }
  | { kind: 'control'; json: unknown }
  | { kind: 'pane'; paneId: string; bytes: Uint8Array }
  | { kind: 'unknown'; tag: number };

/** Encode a JSON-RPC business frame: `0x11 || utf8(JSON.stringify(value))`. */
export function encodeJsonFrame(value: unknown): Uint8Array {
  const body = textEncoder.encode(JSON.stringify(value));
  const out = new Uint8Array(1 + body.length);
  out[0] = CHANNEL.JSON;
  out.set(body, 1);
  return out;
}

/**
 * Encode a session-CONTROL frame (contract §4): `0x12 || utf8(JSON.stringify(value))`.
 * Same JSON encoding as {@link encodeJsonFrame} but on the 0x12 channel, so it is
 * routed to the TOTP handshake handler rather than the JSON-RPC business handler.
 */
export function encodeControlFrame(value: unknown): Uint8Array {
  const body = textEncoder.encode(JSON.stringify(value));
  const out = new Uint8Array(1 + body.length);
  out[0] = CHANNEL.CONTROL;
  out.set(body, 1);
  return out;
}

/**
 * Encode a pane-bytes frame:
 *   `0x10 || paneIdLen(1) || paneId(UTF-8) || rawBytes`.
 * Throws if the paneId exceeds {@link MAX_PANE_ID_BYTES} once UTF-8 encoded.
 */
export function encodePaneFrame(paneId: string, bytes: Uint8Array): Uint8Array {
  const idBytes = textEncoder.encode(paneId);
  if (idBytes.length > MAX_PANE_ID_BYTES) {
    throw new Error(
      `cloudMux: paneId too long (${idBytes.length} > ${MAX_PANE_ID_BYTES} bytes)`,
    );
  }
  const out = new Uint8Array(1 + 1 + idBytes.length + bytes.length);
  out[0] = CHANNEL.PANE_RAW;
  out[1] = idBytes.length;
  out.set(idBytes, 2);
  out.set(bytes, 2 + idBytes.length);
  return out;
}

/**
 * Demux one inbound DataChannel plaintext frame by its leading channel byte.
 * Returns a discriminated result; never throws on a structurally short frame —
 * the caller decides how to surface a malformed frame (the adapter logs +
 * drops, matching the E2EE provider's "reject the bad frame" stance).
 */
export function demuxFrame(frame: Uint8Array): DemuxResult {
  if (frame.length === 0) return { kind: 'unknown', tag: -1 };
  const tag = frame[0];

  if (tag === CHANNEL.JSON) {
    // SECURITY (audit #4): drop oversized JSON frames before the expensive
    // TextDecoder/JSON.parse (match the "drop bad frame" stance — do not throw).
    if (frame.length > MAX_JSON_FRAME_BYTES) return { kind: 'unknown', tag };
    const text = textDecoder.decode(frame.subarray(1));
    const json: unknown = JSON.parse(text); // caller catches parse errors
    return { kind: 'json', json };
  }

  if (tag === CHANNEL.CONTROL) {
    // SECURITY (audit #4): same size cap as the JSON business channel.
    if (frame.length > MAX_JSON_FRAME_BYTES) return { kind: 'unknown', tag };
    const text = textDecoder.decode(frame.subarray(1));
    const json: unknown = JSON.parse(text); // caller catches parse errors
    return { kind: 'control', json };
  }

  if (tag === CHANNEL.PANE_RAW) {
    // SECURITY (audit #4): drop oversized pane-raw frames (larger cap than JSON).
    if (frame.length > MAX_PANE_FRAME_BYTES) return { kind: 'unknown', tag };
    // Need at least the tag + the length byte.
    if (frame.length < 2) return { kind: 'unknown', tag };
    const idLen = frame[1];
    const idEnd = 2 + idLen;
    if (frame.length < idEnd) return { kind: 'unknown', tag };
    const paneId = textDecoder.decode(frame.subarray(2, idEnd));
    // Copy the raw bytes out so the listener owns a standalone buffer (the
    // inbound frame's backing buffer may be reused by the transport).
    const bytes = frame.slice(idEnd);
    return { kind: 'pane', paneId, bytes };
  }

  return { kind: 'unknown', tag };
}
