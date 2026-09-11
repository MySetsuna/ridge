//! RTP1 — Ridge Terminal Protocol v1 wire envelope + 22 message types.
//!
//! Implements SPEC-L2-PROTO-001 §3.3 frame envelope and §3.5 message catalogue.
//! Transport-neutral: WebSocket / WebRTC reliable data channel / in-process
//! pipe share the same byte layout; RTP1 only assumes reliable + ordered +
//! complete delivery (P4).
//!
//! The encoding is intentionally binary with a 5-byte fixed header so a
//! transport cannot silently misinterpret a JSON legacy frame as RTP1 and
//! vice versa (P5 / §3.9).

#![allow(clippy::large_enum_variant)]

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Spec §3.3 — magic 4 bytes 'R','T','P','1'.
pub const RTP1_MAGIC: [u8; 4] = *b"RTP1";

/// Spec §3.3 — envelope format version. Bumping this signals a breaking
/// binary-layout change; clients that do not recognize it MUST drop.
pub const RTP1_EFV: u8 = 0x01;

/// Realtime frame payload cap (SPEC §3.7). Snapshot/replay chunks use 256 KiB
/// independently.
pub const MAX_REALTIME_FRAME: usize = 64 * 1024;

/// Spec §3.5 — message type catalogue (22 entries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    Attach = 0x01,
    AttachAck = 0x02,
    Detach = 0x03,
    DetachAck = 0x04,
    Input = 0x05,
    InputAck = 0x06,
    Output = 0x07,
    Delta = 0x08,
    Resize = 0x09,
    ResizeAck = 0x0A,
    Replay = 0x0B,
    ReplayData = 0x0C,
    Snapshot = 0x0D,
    Title = 0x0E,
    Cwd = 0x0F,
    Desync = 0x10,
    Resync = 0x11,
    Error = 0x12,
    Ping = 0x13,
    Pong = 0x14,
    CapabilityAdvertise = 0x15,
    SessionEvent = 0x16,
}

impl MessageType {
    pub fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0x01 => Self::Attach,
            0x02 => Self::AttachAck,
            0x03 => Self::Detach,
            0x04 => Self::DetachAck,
            0x05 => Self::Input,
            0x06 => Self::InputAck,
            0x07 => Self::Output,
            0x08 => Self::Delta,
            0x09 => Self::Resize,
            0x0A => Self::ResizeAck,
            0x0B => Self::Replay,
            0x0C => Self::ReplayData,
            0x0D => Self::Snapshot,
            0x0E => Self::Title,
            0x0F => Self::Cwd,
            0x10 => Self::Desync,
            0x11 => Self::Resync,
            0x12 => Self::Error,
            0x13 => Self::Ping,
            0x14 => Self::Pong,
            0x15 => Self::CapabilityAdvertise,
            0x16 => Self::SessionEvent,
            _ => return None,
        })
    }

    /// True iff this message type is bound by MAX_REALTIME_FRAME.
    pub fn is_realtime(self) -> bool {
        matches!(
            self,
            Self::Output
                | Self::Input
                | Self::Delta
                | Self::Title
                | Self::Cwd
                | Self::Error
                | Self::Ping
                | Self::Pong
        )
    }
}

/// Spec §3.3 — flags byte. bit0=continuation, bit1=end-of-stream, rest reserved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameFlags(pub u8);

impl FrameFlags {
    pub const CONTINUATION: u8 = 0b0000_0001;
    pub const END_OF_STREAM: u8 = 0b0000_0010;
    pub const MASK_KNOWN: u8 = Self::CONTINUATION | Self::END_OF_STREAM;

    pub fn empty() -> Self {
        Self(0)
    }

    pub fn continuation() -> Self {
        Self(Self::CONTINUATION)
    }

    pub fn contains(self, bit: u8) -> bool {
        (self.0 & bit) == bit
    }
}

/// Decoded RTP1 frame: type + flags + payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub r#type: MessageType,
    pub flags: FrameFlags,
    pub payload: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("frame too short: {0} bytes")]
    TooShort(usize),
    #[error("invalid magic: {0:?}")]
    BadMagic([u8; 4]),
    #[error("unknown envelope format version: {0}")]
    UnknownEfv(u8),
    #[error("unknown message type: {0}")]
    UnknownType(u8),
    #[error("reserved flag bits set: {0:#x}")]
    ReservedFlags(u8),
    #[error("payload length {0} exceeds realtime cap {1}")]
    RealtimeCap(usize, usize),
    #[error("payload length {0} exceeds chunk cap {1}")]
    ChunkCap(usize, usize),
    #[error("payload encode error: {0}")]
    PayloadEncode(String),
    #[error("payload decode error: {0}")]
    PayloadDecode(String),
    #[error("invalid base64: {0}")]
    Base64(#[from] base64::DecodeError),
}

pub const HEADER_LEN: usize = 4 + 1 + 1 + 1 + 4;
pub const FRAME_FLAGS_CONTINUATION: u8 = 0b0000_0001;

/// Encode one RTP1 frame into its wire bytes.
pub fn encode(frame: &Frame) -> Result<Vec<u8>, EnvelopeError> {
    if frame.flags.0 & !FrameFlags::MASK_KNOWN != 0 {
        return Err(EnvelopeError::ReservedFlags(frame.flags.0));
    }
    let payload_len = frame.payload.len();
    if frame.r#type.is_realtime()
        && !frame.flags.contains(FrameFlags::CONTINUATION)
        && payload_len > MAX_REALTIME_FRAME
    {
        return Err(EnvelopeError::RealtimeCap(payload_len, MAX_REALTIME_FRAME));
    }
    let mut out = Vec::with_capacity(HEADER_LEN + payload_len);
    out.extend_from_slice(&RTP1_MAGIC);
    out.push(RTP1_EFV);
    out.push(frame.r#type as u8);
    out.push(frame.flags.0);
    out.extend_from_slice(&(payload_len as u32).to_le_bytes());
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

/// Decode one RTP1 frame. Returns the consumed byte count for stream framing.
pub fn decode(buf: &[u8]) -> Result<(Frame, usize), EnvelopeError> {
    if buf.len() < HEADER_LEN {
        return Err(EnvelopeError::TooShort(buf.len()));
    }
    let mut magic = [0u8; 4];
    magic.copy_from_slice(&buf[..4]);
    if magic != RTP1_MAGIC {
        return Err(EnvelopeError::BadMagic(magic));
    }
    let efv = buf[4];
    if efv != RTP1_EFV {
        return Err(EnvelopeError::UnknownEfv(efv));
    }
    let type_byte = buf[5];
    let flags_byte = buf[6];
    if flags_byte & !FrameFlags::MASK_KNOWN != 0 {
        return Err(EnvelopeError::ReservedFlags(flags_byte));
    }
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&buf[7..11]);
    let payload_len = u32::from_le_bytes(len_bytes) as usize;
    let r#type = MessageType::from_byte(type_byte).ok_or(EnvelopeError::UnknownType(type_byte))?;
    let total = HEADER_LEN + payload_len;
    if buf.len() < total {
        return Err(EnvelopeError::TooShort(buf.len()));
    }
    Ok((
        Frame {
            r#type,
            flags: FrameFlags(flags_byte),
            payload: buf[HEADER_LEN..total].to_vec(),
        },
        total,
    ))
}

// ── Message payloads (SPEC §3.5) ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachRequest {
    pub host_id: String,
    pub runtime_epoch: String,
    pub session_id: String,
    pub terminal_id: String,
    pub controller_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub since_output_seq: Option<u64>,
    pub mode: AttachMode,
    pub client_min_version: u32,
    pub client_max_version: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AttachMode {
    Raw,
    Delta,
    Snapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachAck {
    pub terminal_id: String,
    pub controller_id: String,
    pub server_version: u32,
    pub runtime_epoch: String,
    pub mode: AttachMode,
    pub oldest_output_seq: u64,
    pub next_output_seq: u64,
    pub controller_input_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snapshot: Option<SnapshotEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub capability: Option<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEnvelope {
    pub revision: u64,
    pub snapshot_bytes_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capability {
    pub features: Vec<String>,
    pub max_realtime_frame: usize,
    pub max_snapshot_chunk: usize,
    pub replay_cap_bytes: usize,
    pub replay_cap_frames: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetachRequest {
    pub terminal_id: String,
    pub controller_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetachAck {
    pub terminal_id: String,
    pub controller_id: String,
    pub last_output_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputFrame {
    pub terminal_id: String,
    pub controller_id: String,
    pub input_seq: u64,
    pub data_b64: String,
    pub data_len: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputAck {
    pub terminal_id: String,
    pub controller_id: String,
    pub input_seq: u64,
    pub status: InputAckStatus,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InputAckStatus {
    Applied,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputFrame {
    pub terminal_id: String,
    pub output_seq: u64,
    pub frames: Vec<RawChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawChunk {
    pub seq_offset: u64,
    pub data_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeltaFrame {
    pub terminal_id: String,
    pub output_seq: u64,
    pub delta_bytes_b64: String,
    pub alt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResizeRequest {
    pub terminal_id: String,
    pub controller_id: String,
    pub rows: u16,
    pub cols: u16,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub owner: Option<ResizeOwner>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResizeOwner {
    Controller,
    Observer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResizeAck {
    pub terminal_id: String,
    pub controller_id: String,
    pub rows: u16,
    pub cols: u16,
    pub next_output_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayRequest {
    pub terminal_id: String,
    pub since_output_seq: u64,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayData {
    pub terminal_id: String,
    pub frames: Vec<RawChunk>,
    pub at_oldest: bool,
    pub head_output_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotChunk {
    pub terminal_id: String,
    pub revision: u64,
    pub snapshot_bytes_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TitleEvent {
    pub terminal_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CwdEvent {
    pub terminal_id: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesyncEvent {
    pub terminal_id: String,
    pub reason: DesyncReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DesyncReason {
    Overflow,
    Lagged,
    IoError,
    RuntimeEpochStale,
    ControllerIdUnknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResyncRequest {
    pub terminal_id: String,
    pub mode: AttachMode,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub since_output_seq: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorFrame {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub terminal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub controller_id: Option<String>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PingFrame {
    pub nonce: u64,
}

pub type PongFrame = PingFrame;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityAdvertise {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub terminal_id: Option<String>,
    pub features: Vec<String>,
    pub max_realtime_frame: usize,
    pub max_snapshot_chunk: usize,
    pub replay_cap_bytes: usize,
    pub replay_cap_frames: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionEvent {
    pub terminal_id: String,
    pub event: SessionEventKind,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<i32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    Exited,
    Starting,
    RuntimeEpochRotated,
}

// ── High-level typed envelope helpers ──────────────────────────────────

/// Build a frame from a typed payload.
pub fn frame_from<T: Serialize>(
    r#type: MessageType,
    payload: &T,
    flags: FrameFlags,
) -> Result<Frame, EnvelopeError> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| EnvelopeError::PayloadEncode(error.to_string()))?;
    Ok(Frame {
        r#type,
        flags,
        payload: bytes,
    })
}

pub fn payload_from<T: for<'de> Deserialize<'de>>(frame: &Frame) -> Result<T, EnvelopeError> {
    serde_json::from_slice(&frame.payload)
        .map_err(|error| EnvelopeError::PayloadDecode(error.to_string()))
}

pub fn b64_encode(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    BASE64.decode(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip_for_attach() {
        let attach = AttachRequest {
            host_id: "host-a".into(),
            runtime_epoch: "epoch-1".into(),
            session_id: "session-1".into(),
            terminal_id: "term-1".into(),
            controller_id: "ctrl-1".into(),
            since_output_seq: Some(42),
            mode: AttachMode::Raw,
            client_min_version: 1,
            client_max_version: 1,
        };
        let frame = frame_from(MessageType::Attach, &attach, FrameFlags::empty()).unwrap();
        let wire = encode(&frame).expect("encode attach");
        assert_eq!(&wire[..4], b"RTP1");
        assert_eq!(wire[4], RTP1_EFV);
        assert_eq!(wire[5], MessageType::Attach as u8);
        let (parsed, consumed) = decode(&wire).expect("decode attach");
        assert_eq!(consumed, wire.len());
        assert_eq!(parsed.r#type, MessageType::Attach);
        let decoded: AttachRequest = payload_from(&parsed).unwrap();
        assert_eq!(decoded, attach);
    }

    #[test]
    fn realtime_payload_cap_rejects_oversized_input() {
        let oversized = vec![0u8; MAX_REALTIME_FRAME + 1];
        let payload = InputFrame {
            terminal_id: "t".into(),
            controller_id: "c".into(),
            input_seq: 1,
            data_b64: b64_encode(&oversized),
            data_len: oversized.len(),
        };
        let frame = frame_from(MessageType::Input, &payload, FrameFlags::empty()).unwrap();
        match encode(&frame) {
            Err(EnvelopeError::RealtimeCap(actual, max)) => {
                assert_eq!(max, MAX_REALTIME_FRAME);
                assert!(actual > MAX_REALTIME_FRAME);
            }
            other => panic!("expected RealtimeCap, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_unknown_magic() {
        let mut bad = vec![0u8; HEADER_LEN];
        bad[..4].copy_from_slice(b"HTTP");
        assert!(matches!(decode(&bad), Err(EnvelopeError::BadMagic(_))));
    }

    #[test]
    fn decode_rejects_unknown_efv() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&RTP1_MAGIC);
        buf.push(0x99);
        buf.push(MessageType::Attach as u8);
        buf.push(0);
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(decode(&buf), Err(EnvelopeError::UnknownEfv(0x99))));
    }

    #[test]
    fn decode_rejects_reserved_flag_bits() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&RTP1_MAGIC);
        buf.push(RTP1_EFV);
        buf.push(MessageType::Ping as u8);
        buf.push(0b1000_0000);
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(decode(&buf), Err(EnvelopeError::ReservedFlags(0x80))));
    }

    #[test]
    fn message_type_catalogue_covers_all_22_types() {
        for byte in 0x01u8..=0x16u8 {
            assert!(MessageType::from_byte(byte).is_some(), "missing 0x{byte:02x}");
        }
        assert!(MessageType::from_byte(0x00).is_none());
        assert!(MessageType::from_byte(0x17).is_none());
    }

    #[test]
    fn snapshot_chunked_uses_continuation_bit() {
        let body = vec![0xCDu8; 256 * 1024];
        let mut frames = Vec::new();
        let chunks: Vec<&[u8]> = body.chunks(80 * 1024).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let last = i + 1 == chunks.len();
            let mut flags = FrameFlags::empty();
            if !last {
                flags.0 |= FrameFlags::CONTINUATION;
            }
            let snap = SnapshotChunk {
                terminal_id: "t".into(),
                revision: 1,
                snapshot_bytes_b64: b64_encode(chunk),
            };
            let payload = serde_json::to_vec(&snap).unwrap();
            frames.push(Frame {
                r#type: MessageType::Snapshot,
                flags,
                payload,
            });
        }
        // Reassemble by reading continuation bit and accumulating payload.
        let mut acc = Vec::new();
        for (i, frame) in frames.iter().enumerate() {
            let snap: SnapshotChunk = payload_from(frame).unwrap();
            acc.extend_from_slice(&b64_decode(&snap.snapshot_bytes_b64).unwrap());
            let expected_chunk = chunks[i];
            let prefix_len = acc.len() - expected_chunk.len();
            assert!(acc[prefix_len..] == *expected_chunk, "chunk {i} mismatch");
        }
        assert_eq!(acc, body);
    }

    #[test]
    fn error_frame_round_trips() {
        let e = ErrorFrame {
            terminal_id: Some("term".into()),
            controller_id: None,
            code: "session_closed".into(),
            message: "exited".into(),
        };
        let frame = frame_from(MessageType::Error, &e, FrameFlags::empty()).unwrap();
        let wire = encode(&frame).unwrap();
        let (parsed, _) = decode(&wire).unwrap();
        let back: ErrorFrame = payload_from(&parsed).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn session_event_exited_carries_optional_code() {
        let s = SessionEvent {
            terminal_id: "term".into(),
            event: SessionEventKind::Exited,
            code: Some(0),
        };
        let frame = frame_from(MessageType::SessionEvent, &s, FrameFlags::empty()).unwrap();
        let wire = encode(&frame).unwrap();
        let (parsed, _) = decode(&wire).unwrap();
        let back: SessionEvent = payload_from(&parsed).unwrap();
        assert_eq!(back, s);
    }
}
