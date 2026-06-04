// src/lib/transport/remote/cloudMux.test.ts
//
// Unit tests for the cloud-WebRTC 1-byte channel-prefix mux codec (contract §7).

import { describe, it, expect } from 'vitest';
import {
  CHANNEL,
  MAX_PANE_ID_BYTES,
  demuxFrame,
  encodeControlFrame,
  encodeJsonFrame,
  encodePaneFrame,
} from './cloudMux';

describe('cloudMux — JSON (0x11) framing', () => {
  it('prefixes a JSON frame with 0x11 and UTF-8 body', () => {
    const frame = encodeJsonFrame({ jsonrpc: '2.0', id: 1, method: 'read_file' });
    expect(frame[0]).toBe(CHANNEL.JSON);
    expect(new TextDecoder().decode(frame.subarray(1))).toBe(
      JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'read_file' }),
    );
  });

  it('round-trips a JSON object through encode → demux', () => {
    const value = { jsonrpc: '2.0', method: '$/hello', params: { protocolVersion: 1 } };
    const result = demuxFrame(encodeJsonFrame(value));
    expect(result).toEqual({ kind: 'json', json: value });
  });
});

describe('cloudMux — CONTROL (0x12) framing (§4 TOTP)', () => {
  it('prefixes a control frame with 0x12 and UTF-8 body', () => {
    const frame = encodeControlFrame({ t: 'totp-verify', code: '123456' });
    expect(frame[0]).toBe(CHANNEL.CONTROL);
    expect(new TextDecoder().decode(frame.subarray(1))).toBe(
      JSON.stringify({ t: 'totp-verify', code: '123456' }),
    );
  });

  it('round-trips a control object through encode → demux as kind:control', () => {
    const value = { t: 'totp-result', ok: true };
    expect(demuxFrame(encodeControlFrame(value))).toEqual({ kind: 'control', json: value });
  });

  it('keeps CONTROL (0x12) distinct from the JSON-RPC business channel (0x11)', () => {
    // Same payload on each channel demuxes to different kinds — the gate relies
    // on this separation to allow TOTP while rejecting business frames.
    const payload = { t: 'totp-verify', code: '000000' };
    expect(demuxFrame(encodeControlFrame(payload)).kind).toBe('control');
    expect(demuxFrame(encodeJsonFrame(payload)).kind).toBe('json');
  });
});

describe('cloudMux — PANE_RAW (0x10) framing', () => {
  it('encodes 0x10 || paneIdLen || paneId || bytes', () => {
    const bytes = new Uint8Array([0x1b, 0x5b, 0x41]);
    const frame = encodePaneFrame('pane-7', bytes);
    expect(frame[0]).toBe(CHANNEL.PANE_RAW);
    expect(frame[1]).toBe('pane-7'.length); // ascii → 1 byte each
    expect(new TextDecoder().decode(frame.subarray(2, 2 + frame[1]))).toBe('pane-7');
    expect(frame.subarray(2 + frame[1])).toEqual(bytes);
  });

  it('round-trips paneId + raw bytes through encode → demux', () => {
    const bytes = new Uint8Array([1, 2, 3, 255, 0]);
    const result = demuxFrame(encodePaneFrame('abc', bytes));
    expect(result).toEqual({ kind: 'pane', paneId: 'abc', bytes });
  });

  it('handles a UTF-8 (multi-byte) paneId by byte length, not char length', () => {
    const paneId = '终端-1'; // multi-byte
    const bytes = new Uint8Array([9, 9, 9]);
    const frame = encodePaneFrame(paneId, bytes);
    const idByteLen = new TextEncoder().encode(paneId).length;
    expect(frame[1]).toBe(idByteLen);
    expect(demuxFrame(frame)).toEqual({ kind: 'pane', paneId, bytes });
  });

  it('supports empty raw bytes (header-only pane frame)', () => {
    const result = demuxFrame(encodePaneFrame('p', new Uint8Array()));
    expect(result).toEqual({ kind: 'pane', paneId: 'p', bytes: new Uint8Array() });
  });

  it('throws when the paneId exceeds the 1-byte length limit', () => {
    const tooLong = 'x'.repeat(MAX_PANE_ID_BYTES + 1);
    expect(() => encodePaneFrame(tooLong, new Uint8Array([1]))).toThrow(/too long/);
  });

  it('returns the demuxed bytes as a standalone copy (not a view of the frame)', () => {
    const frame = encodePaneFrame('p', new Uint8Array([7, 8, 9]));
    const result = demuxFrame(frame);
    expect(result.kind).toBe('pane');
    if (result.kind !== 'pane') return;
    // Mutating the original frame buffer must not corrupt the delivered bytes.
    frame.fill(0);
    expect(result.bytes).toEqual(new Uint8Array([7, 8, 9]));
  });
});

describe('cloudMux — demux edge cases', () => {
  it('returns unknown for an empty frame', () => {
    expect(demuxFrame(new Uint8Array())).toEqual({ kind: 'unknown', tag: -1 });
  });

  it('returns unknown for an unrecognised channel tag', () => {
    expect(demuxFrame(new Uint8Array([0x42, 1, 2]))).toEqual({ kind: 'unknown', tag: 0x42 });
  });

  it('returns unknown for a truncated pane frame (missing length byte)', () => {
    expect(demuxFrame(new Uint8Array([CHANNEL.PANE_RAW]))).toEqual({
      kind: 'unknown',
      tag: CHANNEL.PANE_RAW,
    });
  });

  it('returns unknown for a pane frame whose paneId is truncated', () => {
    // claims a 5-byte paneId but only provides 2 bytes after the length prefix
    expect(demuxFrame(new Uint8Array([CHANNEL.PANE_RAW, 5, 0x61, 0x62]))).toEqual({
      kind: 'unknown',
      tag: CHANNEL.PANE_RAW,
    });
  });

  it('throws (caller catches) on malformed JSON', () => {
    const frame = new Uint8Array([CHANNEL.JSON, 0x7b, 0x7b]); // "{{"
    expect(() => demuxFrame(frame)).toThrow();
  });
});
