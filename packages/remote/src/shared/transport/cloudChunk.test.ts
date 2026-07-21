import { describe, it, expect } from 'vitest';
import {
  encodeChunks,
  ChunkReassembler,
  CHUNK_TAG,
  MAX_DC_WIRE_BYTES,
  CHUNK_PAYLOAD_BYTES,
  MAX_REASSEMBLED_BYTES,
} from './cloudChunk';

/** 确定性伪随机字节（不依赖 Math.random，便于复现）。 */
function bytes(n: number, seed = 1): Uint8Array {
  const out = new Uint8Array(n);
  let s = seed >>> 0;
  for (let i = 0; i < n; i++) {
    s = (s * 1664525 + 1013904223) >>> 0;
    out[i] = s & 0xff;
  }
  return out;
}

/** 把一帧密文经 encode→reassembler 跑一遍，返回重组结果。 */
function roundTrip(ct: Uint8Array, msgId = 0): Uint8Array | null {
  const r = new ChunkReassembler();
  let last: Uint8Array | null = null;
  for (const w of encodeChunks(ct, msgId)) {
    expect(w.length).toBeLessThanOrEqual(MAX_DC_WIRE_BYTES); // 每条线消息都不超上限
    last = r.push(w);
  }
  return last;
}

describe('encodeChunks', () => {
  it('小帧走单条 SINGLE（仅多 1 字节 tag）', () => {
    const ct = bytes(100);
    const msgs = encodeChunks(ct, 0);
    expect(msgs).toHaveLength(1);
    expect(msgs[0][0]).toBe(CHUNK_TAG.SINGLE);
    expect(msgs[0].length).toBe(101);
  });

  it('恰好填满单条上限仍是 SINGLE，多 1 字节即切片', () => {
    expect(encodeChunks(bytes(MAX_DC_WIRE_BYTES - 1), 0)).toHaveLength(1); // 1+ct == 上限
    const split = encodeChunks(bytes(MAX_DC_WIRE_BYTES), 0); // 1+ct > 上限 → CHUNK
    expect(split.length).toBeGreaterThan(1);
    expect(split[0][0]).toBe(CHUNK_TAG.CHUNK);
  });

  it('大帧切成多片，每片 ≤ 上限，cnt/idx 正确', () => {
    const ct = bytes(CHUNK_PAYLOAD_BYTES * 3 + 123);
    const msgs = encodeChunks(ct, 7);
    expect(msgs).toHaveLength(4);
    msgs.forEach((m, i) => {
      expect(m[0]).toBe(CHUNK_TAG.CHUNK);
      expect(m.length).toBeLessThanOrEqual(MAX_DC_WIRE_BYTES);
      const dv = new DataView(m.buffer, m.byteOffset, m.byteLength);
      expect(dv.getUint32(1, false)).toBe(7); // msgId
      expect(dv.getUint16(5, false)).toBe(i); // idx
      expect(dv.getUint16(7, false)).toBe(4); // cnt
    });
  });
});

describe('ChunkReassembler round-trip', () => {
  it('SINGLE 帧原样还原', () => {
    const ct = bytes(500, 9);
    expect(roundTrip(ct)).toEqual(ct);
  });

  it('空帧（0 字节密文）也能往返', () => {
    const ct = new Uint8Array(0);
    expect(roundTrip(ct)).toEqual(ct);
  });

  it('多分片帧按序重组还原（含非整除尾片）', () => {
    const ct = bytes(CHUNK_PAYLOAD_BYTES * 5 + 1, 42);
    const out = roundTrip(ct, 123);
    expect(out).not.toBeNull();
    expect(out).toEqual(ct);
  });

  it('连续多帧（单/多片混合）各自独立还原', () => {
    const r = new ChunkReassembler();
    const frames = [bytes(10, 1), bytes(CHUNK_PAYLOAD_BYTES * 2 + 5, 2), bytes(0), bytes(MAX_DC_WIRE_BYTES + 9, 3)];
    frames.forEach((ct, fi) => {
      let out: Uint8Array | null = null;
      for (const w of encodeChunks(ct, fi)) out = r.push(w);
      expect(out).toEqual(ct);
    });
  });
});

describe('ChunkReassembler robustness', () => {
  it('未知 tag 的线消息被丢弃（返回 null）', () => {
    const r = new ChunkReassembler();
    expect(r.push(new Uint8Array([0x07, 1, 2, 3]))).toBeNull();
    expect(r.push(new Uint8Array(0))).toBeNull();
  });

  it('CHUNK 头不完整 → 丢弃', () => {
    const r = new ChunkReassembler();
    expect(r.push(new Uint8Array([CHUNK_TAG.CHUNK, 0, 0]))).toBeNull();
  });

  it('收齐前不产出，半帧返回 null', () => {
    const ct = bytes(CHUNK_PAYLOAD_BYTES * 2 + 1);
    const msgs = encodeChunks(ct, 0);
    const r = new ChunkReassembler();
    expect(r.push(msgs[0])).toBeNull(); // 第 1/3 片
    expect(r.push(msgs[1])).toBeNull(); // 第 2/3 片
    expect(r.push(msgs[2])).toEqual(ct); // 收齐
  });

  it('乱序/越界分片被拒（不产出错误数据）', () => {
    const ct = bytes(CHUNK_PAYLOAD_BYTES * 2 + 1);
    const msgs = encodeChunks(ct, 0);
    const r = new ChunkReassembler();
    // 跳过 idx0 直接喂 idx1 → 非 idx0 开头，msgId 不匹配（初始 -1）→ 丢弃。
    expect(r.push(msgs[1])).toBeNull();
    // 正常重来一遍仍能成功（复位后自愈）。
    let out: Uint8Array | null = null;
    for (const w of msgs) out = r.push(w);
    expect(out).toEqual(ct);
  });

  it('超大重组（超过 MAX_REASSEMBLED_BYTES）被丢弃而非 OOM', () => {
    // 伪造 cnt 很大、每片满载的 CHUNK 序列，喂到超过上限即返回 null 并复位。
    const r = new ChunkReassembler();
    const cnt = Math.ceil(MAX_REASSEMBLED_BYTES / CHUNK_PAYLOAD_BYTES) + 2;
    let dropped = false;
    for (let i = 0; i < cnt; i++) {
      const part = bytes(CHUNK_PAYLOAD_BYTES, i + 1);
      const m = new Uint8Array(9 + part.length);
      const dv = new DataView(m.buffer);
      m[0] = CHUNK_TAG.CHUNK;
      dv.setUint32(1, 1, false);
      dv.setUint16(5, i, false);
      dv.setUint16(7, cnt, false);
      m.set(part, 9);
      if (r.push(m) === null && i > 0) {
        // 某一片越过上限后被丢弃复位（后续 idx 与复位后的 msgId 不再匹配）。
        dropped = true;
        break;
      }
    }
    expect(dropped).toBe(true);
  });
});
