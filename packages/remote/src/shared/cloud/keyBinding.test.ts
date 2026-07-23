// Unit tests for the E2EE key-binding verifier core (D-GM-10 / B3).
// The security-critical property: a swapped/tampered peer pubkey (the relay-MITM
// signature) is REJECTED; a matching one is accepted; and the compat path
// (e2ee-bind not negotiated) stays permissive so old controllers don't regress.

import { describe, it, expect } from 'vitest';
import { constantTimeEqual, decideKeyBinding } from './keyBinding';
import { PUBKEY_LEN } from './e2ee';

function pubkey(fill: number): Uint8Array {
  return new Uint8Array(PUBKEY_LEN).fill(fill);
}

describe('constantTimeEqual', () => {
  it('returns true for equal byte arrays', () => {
    expect(constantTimeEqual(pubkey(7), pubkey(7))).toBe(true);
    expect(constantTimeEqual(new Uint8Array([]), new Uint8Array([]))).toBe(true);
  });

  it('returns false for differing contents of equal length', () => {
    const a = pubkey(1);
    const b = pubkey(1);
    b[31] = 2; // single trailing-byte difference
    expect(constantTimeEqual(a, b)).toBe(false);
    const c = pubkey(1);
    c[0] = 2; // single leading-byte difference (must not early-return)
    expect(constantTimeEqual(a, c)).toBe(false);
  });

  it('returns false for differing lengths', () => {
    expect(constantTimeEqual(new Uint8Array([1, 2, 3]), new Uint8Array([1, 2]))).toBe(false);
  });
});

// makeKeyBindingVerifier 已随 §5.5 桥钩子退役删除（S1-F5）；其 MITM 比对语义由
// decideKeyBinding（下）在双 provider 生产路径覆盖。

describe('decideKeyBinding — signaling-presence gate (the live B3 decision)', () => {
  it('ACCEPTS when handshake pubkey matches the signaling pubkey', () => {
    expect(decideKeyBinding(pubkey(0xab), pubkey(0xab), false)).toBe('accept');
  });

  it('REJECTS when handshake pubkey differs from the signaling pubkey (MITM)', () => {
    // A DataChannel MITM swapped the handshake pubkey, but it can't touch the
    // pubkey relayed over the separate authenticated TLS signaling → mismatch.
    expect(decideKeyBinding(pubkey(0xcd), pubkey(0xab), false)).toBe('reject');
    expect(decideKeyBinding(pubkey(0xcd), pubkey(0xab), true)).toBe('reject');
  });

  it('WAITS when the signaling pubkey has not arrived yet and grace has not expired', () => {
    expect(decideKeyBinding(pubkey(0xab), null, false)).toBe('wait');
  });

  it('falls back to relay-trust (ACCEPT) once grace expires without a signaling pubkey', () => {
    // The peer is an old client that never sends its signaling pubkey — a
    // DataChannel MITM cannot induce this path (it can't suppress the separate
    // TLS signaling), so relay-trust here is the backward-compat case only.
    expect(decideKeyBinding(pubkey(0xab), null, true)).toBe('accept');
  });

  it('REJECTS defensively on illegal pubkey length when a signaling pubkey is present', () => {
    const short = new Uint8Array(PUBKEY_LEN - 1).fill(0xab);
    expect(decideKeyBinding(short, pubkey(0xab), false)).toBe('reject');
    expect(decideKeyBinding(pubkey(0xab), short, false)).toBe('reject');
  });
});
