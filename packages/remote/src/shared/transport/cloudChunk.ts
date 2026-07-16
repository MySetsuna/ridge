// src/lib/transport/remote/cloudChunk.ts
//
// WebRTC DataChannel 传输层分片（契约 §7 传输细节）。修复 cloud 远控在大帧上的
// `RTCDataChannel.send: Trying to send message larger than max-message-size`：
// 一条 SCTP DataChannel 消息有协商出的 `maxMessageSize` 上限（Chrome 约 256 KiB，
// 移动 Safari / TURN/TCP 链路可能更低）。E2EE 业务帧（如 read_file/get_file_tree/
// text_search 结果、大文件 write_file、PTY scrollback 回放）的**密文**一旦超过该
// 上限，`dc.send()` 会**同步抛异常**，整条业务帧丢失。
//
// 本模块把「已 seal 的密文」在**裸 DataChannel 字节层**切成 ≤16 KiB 的小消息发送、
// 接收端按序重组回完整密文再交给 `session.open()`——E2EE seal/open 仍是**每帧一次**，
// nonce 计数与抗重放不变（只切传输、不切加密）。
//
// 适用范围（重要）：仅 **握手完成后**的业务帧经本封装。§7.1 握手帧（0x01||pub32 /
// 0x02 签名帧）在握手阶段裸发裸收，**不**过本封装（收端在 handshakeDone 之前直接按
// 握手帧解析）。故 0x00/0x01 这两个传输 tag 只在握手后被解释，与握手帧首字节不冲突。
//
// 线格式（握手后每条 DataChannel 消息）：
//   0x00  SINGLE : `0x00 || ciphertext`                         （整帧,≤16KiB-1）
//   0x01  CHUNK  : `0x01 || msgId(u32 BE) || idx(u16 BE) || cnt(u16 BE) || part`
// `ordered:true` 保证分片按序到达，故同一 msgId 的分片连续、不与别的 msgId 交错。

/** 传输分片 tag（仅握手后解释）。 */
export const CHUNK_TAG = { SINGLE: 0x00, CHUNK: 0x01 } as const;

/** CHUNK 头长度：tag(1) + msgId(4) + idx(2) + cnt(2)。 */
const CHUNK_HEADER_BYTES = 1 + 4 + 2 + 2; // 9

/**
 * 单条 DataChannel 消息的硬上限（含 tag/头）。16 KiB 是跨浏览器/跨链路都安全的 SCTP
 * 消息尺寸（远低于 Chrome 256 KiB，也覆盖移动 Safari 与 turns/TCP 受限路径）。所有产出的
 * 线消息都 ≤ 此值，故任意对端 `maxMessageSize` 下都不会触发「message too large」。
 */
export const MAX_DC_WIRE_BYTES = 16 * 1024;

/** CHUNK 每片最大业务字节（保证整条线消息 ≤ {@link MAX_DC_WIRE_BYTES}）。 */
export const CHUNK_PAYLOAD_BYTES = MAX_DC_WIRE_BYTES - CHUNK_HEADER_BYTES; // 16375

/**
 * 重组密文的硬上限（防恶意对端用海量分片撑爆内存）。与入站明文上限（16 MiB，见
 * cloudMux `MAX_PANE_FRAME_BYTES`）对齐并留 E2EE 开销余量；超限即丢弃并复位。
 */
export const MAX_REASSEMBLED_BYTES = 16 * 1024 * 1024 + 4096;

/**
 * 把一帧已 seal 的密文编码为 1+ 条 DataChannel 线消息。
 *   - 密文 + 1 字节 tag ≤ {@link MAX_DC_WIRE_BYTES} → 单条 SINGLE（常见路径，仅多 1 字节）。
 *   - 否则按 {@link CHUNK_PAYLOAD_BYTES} 切成多条 CHUNK（msgId 标识本帧，idx/cnt 供重组）。
 *
 * @param ciphertext 已 seal 的密文（`E2eeSession.seal` 的输出）。
 * @param msgId      本帧的递增标识（每帧自增，u32 回绕无害——ordered 下不会并发重组）。
 */
export function encodeChunks(ciphertext: Uint8Array, msgId: number): Uint8Array[] {
  if (1 + ciphertext.length <= MAX_DC_WIRE_BYTES) {
    const out = new Uint8Array(1 + ciphertext.length);
    out[0] = CHUNK_TAG.SINGLE;
    out.set(ciphertext, 1);
    return [out];
  }
  const count = Math.ceil(ciphertext.length / CHUNK_PAYLOAD_BYTES);
  const id = msgId >>> 0;
  const out: Uint8Array[] = [];
  for (let i = 0; i < count; i++) {
    const start = i * CHUNK_PAYLOAD_BYTES;
    const part = ciphertext.subarray(start, start + CHUNK_PAYLOAD_BYTES);
    const msg = new Uint8Array(CHUNK_HEADER_BYTES + part.length);
    const dv = new DataView(msg.buffer);
    msg[0] = CHUNK_TAG.CHUNK;
    dv.setUint32(1, id, false);
    dv.setUint16(5, i, false);
    dv.setUint16(7, count, false);
    msg.set(part, CHUNK_HEADER_BYTES);
    out.push(msg);
  }
  return out;
}

/**
 * 按序重组器（每条连接一个；重连/断开时 {@link reset}）。把入站 DataChannel 线消息
 * 还原为完整密文：SINGLE 立即产出；CHUNK 累积到收齐再产出。永不抛错——结构性坏帧/
 * 超限一律丢弃（返回 null）并复位，与 provider「拒绝坏帧但不一定断连」立场一致。
 */
export class ChunkReassembler {
  private msgId = -1;
  private count = 0;
  private received = 0;
  private parts: Array<Uint8Array | undefined> = [];
  private total = 0;

  /**
   * 喂入一条握手后的入站线消息。
   * @returns 收齐一帧 → 完整密文；半帧/坏帧/超限 → null（半帧继续等，坏帧已丢弃）。
   */
  push(wire: Uint8Array): Uint8Array | null {
    if (wire.length === 0) return null;
    const tag = wire[0];

    if (tag === CHUNK_TAG.SINGLE) {
      this.reset(); // 任何在途分片遇到 SINGLE 视为异常中断，丢弃后产出本整帧
      return wire.slice(1);
    }
    if (tag !== CHUNK_TAG.CHUNK || wire.length < CHUNK_HEADER_BYTES) {
      this.reset();
      return null;
    }

    const dv = new DataView(wire.buffer, wire.byteOffset, wire.byteLength);
    const msgId = dv.getUint32(1, false);
    const idx = dv.getUint16(5, false);
    const cnt = dv.getUint16(7, false);
    const part = wire.subarray(CHUNK_HEADER_BYTES);

    // idx==0 开新帧：（重）初始化累积态。ordered 下分片严格按 0,1,2… 到达。
    if (idx === 0) {
      this.msgId = msgId;
      this.count = cnt;
      this.received = 0;
      this.parts = new Array(cnt);
      this.total = 0;
    }

    // 连续性校验：msgId/cnt 一致、idx 在界内、该片未重复。任一不符 → 丢弃复位。
    if (
      cnt === 0 ||
      msgId !== this.msgId ||
      cnt !== this.count ||
      idx >= this.count ||
      this.parts[idx] !== undefined
    ) {
      this.reset();
      return null;
    }

    this.total += part.length;
    if (this.total > MAX_REASSEMBLED_BYTES) {
      this.reset();
      return null;
    }
    this.parts[idx] = part.slice(); // 拷出独立 buffer（传输底层 buffer 可能被复用）
    this.received += 1;
    if (this.received < this.count) return null;

    // 收齐 → 拼接产出。
    const full = new Uint8Array(this.total);
    let off = 0;
    for (const p of this.parts) {
      if (!p) {
        // 防御：理论不达（received==count 即全到），但若有空洞则丢弃整帧。
        this.reset();
        return null;
      }
      full.set(p, off);
      off += p.length;
    }
    this.reset();
    return full;
  }

  /** 复位累积态（重连/断开/坏帧）。 */
  reset(): void {
    this.msgId = -1;
    this.count = 0;
    this.received = 0;
    this.parts = [];
    this.total = 0;
  }
}
