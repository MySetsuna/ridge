// Ridge Cloud — RemoteConnectionProvider 接口（契约 §0 拓扑、§7 E2EE 帧）
//
// 这是云端公网加速通道的抽象传输层。它与既有 LAN 远控（src/remote/lib/wsRemote.ts）
// 是**并行**的 provider，不替换 LAN 逻辑。
//
// 设计要点（契约 §7.2）：
//   - 内层明文 = 既有 postcard 二进制增量帧（PTY 输出 / resize / 输入等），
//     schema 不变。Provider 只负责把这些 *opaque* 二进制帧加密后经 WebRTC
//     DataChannel 收发，不解析 postcard。
//   - 加解密发生在 provider 内部（DataChannel 之上再叠一层，见 e2ee.ts），
//     调用方拿到的 onFrame 回调已是解密后的明文字节。

/** Provider 生命周期状态。与 LAN 端 ConnectionState 语义对齐再加 e2ee 阶段。 */
export type CloudConnectionState =
  | 'disconnected'
  | 'connecting' // 取 ICE、连 WS、ICE/DTLS 协商中
  | 'handshaking' // DataChannel 已 open，正在跑 X25519 E2EE 握手
  | 'connected' // E2EE 握手完成，业务帧可收发
  | 'error';

export interface CloudConnectionCallbacks {
  /** 状态变化（驱动 UI）。 */
  onState?: (state: CloudConnectionState) => void;
  /** 出错（结构化错误 + 人类可读信息）。 */
  onError?: (message: string, code?: string) => void;
  /**
   * 收到**已解密**的业务帧（明文 postcard 字节）。调用方按既有 schema 解析。
   */
  onFrame?: (plaintext: Uint8Array) => void;
}

/**
 * 远程连接 provider 抽象。云端实现见 ridgeCloudProvider.ts。
 *
 * 约定：
 *   - connect(deviceId) 建立到该设备租户的端到端加密通道。
 *   - sendFrame(bytes) 把一帧明文 postcard 字节加密后经 DataChannel 发出。
 *     仅在 state === 'connected'（E2EE 握手完成）后可用，否则静默丢弃。
 *   - disconnect() 幂等关闭全部资源（WS / RTCPeerConnection / DataChannel）。
 */
export interface RemoteConnectionProvider {
  /** 连接到指定 deviceId（device_name）的租户通道。 */
  connect(deviceId: string): Promise<void>;
  /** 关闭连接并释放资源（幂等）。 */
  disconnect(): void;
  /** 发送一帧明文业务字节（内部加密）。 */
  sendFrame(plaintext: Uint8Array): void;
  /** 当前状态。 */
  getState(): CloudConnectionState;
}
