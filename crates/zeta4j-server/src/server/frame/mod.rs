// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # Frame Protocol Module
//!
//! Phase 2 Step 1 - M1.4: 프레임 프로토콜
//!
//! ## Features
//! - 메시지 프레이밍 (길이 접두 프레임)
//! - 청크 전송 (대용량 데이터 스트리밍)
//! - LZ4 압축 지원
//! - 프레임 타입별 처리

mod chunk;
mod codec;
mod compress;

pub use chunk::{ChunkConfig, ChunkMetadata, ChunkProgress, ChunkedReader, ChunkedWriter};
pub use codec::{FrameCodec, FrameDecoder, FrameEncoder};
pub use compress::{CompressionLevel, CompressionStats, Compressor, Decompressor};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::fmt;

/// 프레임 프로토콜 에러
#[derive(Debug, Clone)]
pub enum FrameError {
    /// 불완전한 프레임
    Incomplete,
    /// 프레임 크기 초과
    TooLarge { size: usize, max: usize },
    /// 잘못된 프레임 타입
    InvalidType(u8),
    /// 잘못된 매직 넘버
    InvalidMagic,
    /// 체크섬 불일치
    ChecksumMismatch { expected: u32, actual: u32 },
    /// 압축 해제 실패
    DecompressionFailed(String),
    /// IO 에러
    Io(String),
    /// 프로토콜 에러
    Protocol(String),
}

impl std::error::Error for FrameError {}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete => write!(f, "Incomplete frame"),
            Self::TooLarge { size, max } => {
                write!(f, "Frame too large: {} bytes (max: {})", size, max)
            }
            Self::InvalidType(t) => write!(f, "Invalid frame type: {}", t),
            Self::InvalidMagic => write!(f, "Invalid magic number"),
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "Checksum mismatch: expected {}, got {}",
                    expected, actual
                )
            }
            Self::DecompressionFailed(e) => write!(f, "Decompression failed: {}", e),
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Protocol(e) => write!(f, "Protocol error: {}", e),
        }
    }
}

impl From<std::io::Error> for FrameError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// 매직 넘버 (ZETA)
pub const MAGIC: u32 = 0x5A455441;

/// 프레임 헤더 크기 (12 bytes)
/// - Magic: 4 bytes
/// - Type: 1 byte
/// - Flags: 1 byte
/// - Reserved: 2 bytes
/// - Length: 4 bytes
pub const HEADER_SIZE: usize = 12;

/// 기본 최대 프레임 크기 (16MB)
pub const DEFAULT_MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// 프레임 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameType {
    /// 일반 데이터 프레임
    Data = 0x00,
    /// 청크 시작
    ChunkStart = 0x01,
    /// 청크 데이터
    ChunkData = 0x02,
    /// 청크 종료
    ChunkEnd = 0x03,
    /// Ping (연결 확인)
    Ping = 0x10,
    /// Pong (Ping 응답)
    Pong = 0x11,
    /// GoAway (연결 종료 예고)
    GoAway = 0x20,
    /// Reset (스트림 리셋)
    Reset = 0x21,
    /// Settings (설정 교환)
    Settings = 0x30,
    /// SettingsAck (설정 확인)
    SettingsAck = 0x31,
    /// Error (에러 프레임)
    Error = 0xFF,
}

impl FrameType {
    /// 바이트에서 프레임 타입으로 변환
    pub fn from_u8(value: u8) -> Result<Self, FrameError> {
        match value {
            0x00 => Ok(Self::Data),
            0x01 => Ok(Self::ChunkStart),
            0x02 => Ok(Self::ChunkData),
            0x03 => Ok(Self::ChunkEnd),
            0x10 => Ok(Self::Ping),
            0x11 => Ok(Self::Pong),
            0x20 => Ok(Self::GoAway),
            0x21 => Ok(Self::Reset),
            0x30 => Ok(Self::Settings),
            0x31 => Ok(Self::SettingsAck),
            0xFF => Ok(Self::Error),
            _ => Err(FrameError::InvalidType(value)),
        }
    }

    /// 컨트롤 프레임 여부
    pub fn is_control(&self) -> bool {
        matches!(
            self,
            Self::Ping
                | Self::Pong
                | Self::GoAway
                | Self::Reset
                | Self::Settings
                | Self::SettingsAck
        )
    }

    /// 청크 관련 프레임 여부
    pub fn is_chunk(&self) -> bool {
        matches!(self, Self::ChunkStart | Self::ChunkData | Self::ChunkEnd)
    }
}

impl fmt::Display for FrameType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data => write!(f, "DATA"),
            Self::ChunkStart => write!(f, "CHUNK_START"),
            Self::ChunkData => write!(f, "CHUNK_DATA"),
            Self::ChunkEnd => write!(f, "CHUNK_END"),
            Self::Ping => write!(f, "PING"),
            Self::Pong => write!(f, "PONG"),
            Self::GoAway => write!(f, "GO_AWAY"),
            Self::Reset => write!(f, "RESET"),
            Self::Settings => write!(f, "SETTINGS"),
            Self::SettingsAck => write!(f, "SETTINGS_ACK"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

bitflags::bitflags! {
    /// 프레임 플래그
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FrameFlags: u8 {
        /// 압축됨 (LZ4)
        const COMPRESSED = 0b0000_0001;
        /// 스트림 종료
        const END_STREAM = 0b0000_0100;
        /// 우선순위 포함
        const PRIORITY = 0b0000_1000;
        /// 패딩 포함
        const PADDED = 0b0001_0000;
        /// ACK 요청
        const ACK = 0b0010_0000;
    }
}

impl Default for FrameFlags {
    fn default() -> Self {
        Self::empty()
    }
}

/// 프레임 헤더
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// 프레임 타입
    pub frame_type: FrameType,
    /// 프레임 플래그
    pub flags: FrameFlags,
    /// 페이로드 길이
    pub length: u32,
    /// 스트림 ID (0 = 연결 레벨)
    pub stream_id: u16,
}

impl FrameHeader {
    /// 새 프레임 헤더 생성
    pub fn new(frame_type: FrameType, length: u32) -> Self {
        Self {
            frame_type,
            flags: FrameFlags::empty(),
            length,
            stream_id: 0,
        }
    }

    /// 플래그 설정
    pub fn with_flags(mut self, flags: FrameFlags) -> Self {
        self.flags = flags;
        self
    }

    /// 스트림 ID 설정
    pub fn with_stream_id(mut self, stream_id: u16) -> Self {
        self.stream_id = stream_id;
        self
    }

    /// 바이트로 인코딩
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u32(MAGIC);
        buf.put_u8(self.frame_type as u8);
        buf.put_u8(self.flags.bits());
        buf.put_u16(self.stream_id);
        buf.put_u32(self.length);
    }

    /// 바이트에서 디코딩
    pub fn decode(buf: &mut impl Buf) -> Result<Self, FrameError> {
        if buf.remaining() < HEADER_SIZE {
            return Err(FrameError::Incomplete);
        }

        let magic = buf.get_u32();
        if magic != MAGIC {
            return Err(FrameError::InvalidMagic);
        }

        let frame_type = FrameType::from_u8(buf.get_u8())?;
        let flags = FrameFlags::from_bits_truncate(buf.get_u8());
        let stream_id = buf.get_u16();
        let length = buf.get_u32();

        Ok(Self {
            frame_type,
            flags,
            length,
            stream_id,
        })
    }

    /// 압축 플래그 여부
    pub fn is_compressed(&self) -> bool {
        self.flags.contains(FrameFlags::COMPRESSED)
    }

    /// 스트림 종료 플래그 여부
    pub fn is_end_stream(&self) -> bool {
        self.flags.contains(FrameFlags::END_STREAM)
    }
}

/// 프레임
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 프레임 헤더
    pub header: FrameHeader,
    /// 페이로드
    pub payload: Bytes,
}

impl Frame {
    /// 새 프레임 생성
    pub fn new(frame_type: FrameType, payload: impl Into<Bytes>) -> Self {
        let payload = payload.into();
        Self {
            header: FrameHeader::new(frame_type, payload.len() as u32),
            payload,
        }
    }

    /// 데이터 프레임 생성
    pub fn data(payload: impl Into<Bytes>) -> Self {
        Self::new(FrameType::Data, payload)
    }

    /// Ping 프레임 생성
    pub fn ping(payload: impl Into<Bytes>) -> Self {
        Self::new(FrameType::Ping, payload)
    }

    /// Pong 프레임 생성
    pub fn pong(payload: impl Into<Bytes>) -> Self {
        Self::new(FrameType::Pong, payload)
    }

    /// GoAway 프레임 생성
    pub fn go_away(error_code: u32, message: &str) -> Self {
        let mut payload = BytesMut::with_capacity(4 + message.len());
        payload.put_u32(error_code);
        payload.put_slice(message.as_bytes());
        Self::new(FrameType::GoAway, payload.freeze())
    }

    /// Reset 프레임 생성
    pub fn reset(error_code: u32) -> Self {
        let mut payload = BytesMut::with_capacity(4);
        payload.put_u32(error_code);
        Self::new(FrameType::Reset, payload.freeze())
    }

    /// Error 프레임 생성
    pub fn error(error_code: u32, message: &str) -> Self {
        let mut payload = BytesMut::with_capacity(4 + message.len());
        payload.put_u32(error_code);
        payload.put_slice(message.as_bytes());
        Self::new(FrameType::Error, payload.freeze())
    }

    /// Settings 프레임 생성
    pub fn settings(settings: &[(u16, u32)]) -> Self {
        let mut payload = BytesMut::with_capacity(settings.len() * 6);
        for (key, value) in settings {
            payload.put_u16(*key);
            payload.put_u32(*value);
        }
        Self::new(FrameType::Settings, payload.freeze())
    }

    /// SettingsAck 프레임 생성
    pub fn settings_ack() -> Self {
        let mut frame = Self::new(FrameType::SettingsAck, Bytes::new());
        frame.header.flags = FrameFlags::ACK;
        frame
    }

    /// 플래그 설정
    pub fn with_flags(mut self, flags: FrameFlags) -> Self {
        self.header.flags = flags;
        self
    }

    /// 스트림 ID 설정
    pub fn with_stream_id(mut self, stream_id: u16) -> Self {
        self.header.stream_id = stream_id;
        self
    }

    /// 프레임 타입 반환
    pub fn frame_type(&self) -> FrameType {
        self.header.frame_type
    }

    /// 페이로드 길이
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }

    /// 전체 프레임 크기 (헤더 + 페이로드)
    pub fn total_size(&self) -> usize {
        HEADER_SIZE + self.payload.len()
    }

    /// 바이트로 인코딩
    pub fn encode(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(self.total_size());
        self.header.encode(&mut buf);
        buf.put_slice(&self.payload);
        buf
    }

    /// 바이트에서 디코딩
    pub fn decode(buf: &mut BytesMut, max_size: usize) -> Result<Option<Self>, FrameError> {
        if buf.len() < HEADER_SIZE {
            return Ok(None);
        }

        // 헤더 미리보기 (consume 하지 않음)
        let header = {
            let mut peek = buf.clone();
            FrameHeader::decode(&mut peek)?
        };

        let total_size = HEADER_SIZE + header.length as usize;

        // 최대 크기 체크
        if total_size > max_size {
            return Err(FrameError::TooLarge {
                size: total_size,
                max: max_size,
            });
        }

        // 전체 프레임이 아직 도착하지 않음
        if buf.len() < total_size {
            return Ok(None);
        }

        // 헤더 consume
        buf.advance(HEADER_SIZE);

        // 페이로드 추출
        let payload = buf.split_to(header.length as usize).freeze();

        Ok(Some(Self { header, payload }))
    }
}

/// 프레임 설정
#[derive(Debug, Clone)]
pub struct FrameConfig {
    /// 최대 프레임 크기
    pub max_frame_size: usize,
    /// 압축 활성화
    pub compression_enabled: bool,
    /// 압축 임계값 (이 크기 이상일 때 압축)
    pub compression_threshold: usize,
    /// 압축 레벨
    pub compression_level: CompressionLevel,
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self {
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            compression_enabled: true,
            compression_threshold: 1024, // 1KB 이상일 때 압축
            compression_level: CompressionLevel::Default,
        }
    }
}

impl FrameConfig {
    /// 빌더 생성
    pub fn builder() -> FrameConfigBuilder {
        FrameConfigBuilder::default()
    }
}

/// 프레임 설정 빌더
#[derive(Debug, Default)]
pub struct FrameConfigBuilder {
    config: FrameConfig,
}

impl FrameConfigBuilder {
    /// 최대 프레임 크기 설정
    pub fn max_frame_size(mut self, size: usize) -> Self {
        self.config.max_frame_size = size;
        self
    }

    /// 압축 활성화 설정
    pub fn compression_enabled(mut self, enabled: bool) -> Self {
        self.config.compression_enabled = enabled;
        self
    }

    /// 압축 임계값 설정
    pub fn compression_threshold(mut self, threshold: usize) -> Self {
        self.config.compression_threshold = threshold;
        self
    }

    /// 압축 레벨 설정
    pub fn compression_level(mut self, level: CompressionLevel) -> Self {
        self.config.compression_level = level;
        self
    }

    /// 빌드
    pub fn build(self) -> FrameConfig {
        self.config
    }
}
