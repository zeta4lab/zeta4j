//! # Frame Codec Module
//!
//! 프레임 인코딩/디코딩 코덱을 제공합니다.
//!
//! ## Features
//! - tokio_util::codec 호환
//! - 자동 압축/해제
//! - 스트리밍 처리

#![allow(dead_code)]

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io;
use tokio_util::codec::{Decoder, Encoder};

use super::compress::{Compressor, Decompressor};
use super::{Frame, FrameConfig, FrameError, FrameFlags, FrameHeader, FrameType, HEADER_SIZE};

/// 프레임 코덱
///
/// tokio_util::codec의 Encoder/Decoder를 구현하여
/// 비동기 스트림에서 프레임을 처리합니다.
#[derive(Debug, Clone)]
pub struct FrameCodec {
    config: FrameConfig,
    compressor: Option<Compressor>,
    decompressor: Decompressor,
}

impl Default for FrameCodec {
    fn default() -> Self {
        Self::new(FrameConfig::default())
    }
}

impl FrameCodec {
    /// 새 프레임 코덱 생성
    pub fn new(config: FrameConfig) -> Self {
        let compressor = if config.compression_enabled {
            Some(
                Compressor::new(config.compression_level)
                    .with_threshold(config.compression_threshold),
            )
        } else {
            None
        };

        Self {
            decompressor: Decompressor::new().with_max_size(config.max_frame_size),
            config,
            compressor,
        }
    }

    /// 압축 없이 생성
    pub fn without_compression() -> Self {
        Self::new(FrameConfig {
            compression_enabled: false,
            ..Default::default()
        })
    }

    /// 설정 조회
    pub fn config(&self) -> &FrameConfig {
        &self.config
    }

    /// 프레임 인코딩 (압축 적용)
    fn encode_frame(&self, frame: &Frame) -> Result<BytesMut, FrameError> {
        let (payload, compressed) = if let Some(ref compressor) = self.compressor {
            // 데이터 프레임만 압축
            if frame.frame_type() == FrameType::Data && frame.payload_len() > 0 {
                if let Some(compressed) = compressor.compress(&frame.payload) {
                    (compressed, true)
                } else {
                    (frame.payload.clone(), false)
                }
            } else {
                (frame.payload.clone(), false)
            }
        } else {
            (frame.payload.clone(), false)
        };

        let mut header = frame.header;
        header.length = payload.len() as u32;
        if compressed {
            header.flags |= FrameFlags::COMPRESSED;
        }

        let mut buf = BytesMut::with_capacity(HEADER_SIZE + payload.len());
        header.encode(&mut buf);
        buf.put_slice(&payload);

        Ok(buf)
    }

    /// 프레임 디코딩 (압축 해제)
    fn decode_frame(&self, header: FrameHeader, payload: Bytes) -> Result<Frame, FrameError> {
        let payload = if header.is_compressed() {
            self.decompressor.decompress(&payload)?
        } else {
            payload
        };

        Ok(Frame {
            header: FrameHeader {
                frame_type: header.frame_type,
                flags: header.flags & !FrameFlags::COMPRESSED, // 압축 플래그 제거
                length: payload.len() as u32,
                stream_id: header.stream_id,
            },
            payload,
        })
    }
}

impl Decoder for FrameCodec {
    type Item = Frame;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // 헤더가 완전히 도착했는지 확인
        if src.len() < HEADER_SIZE {
            return Ok(None);
        }

        // 헤더 미리보기
        let header = {
            let mut peek = src.clone();
            match FrameHeader::decode(&mut peek) {
                Ok(h) => h,
                Err(FrameError::Incomplete) => return Ok(None),
                Err(FrameError::InvalidMagic) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid magic number",
                    ));
                }
                Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
            }
        };

        let total_size = HEADER_SIZE + header.length as usize;

        // 최대 크기 체크
        if total_size > self.config.max_frame_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Frame too large: {} bytes (max: {})",
                    total_size, self.config.max_frame_size
                ),
            ));
        }

        // 전체 프레임이 도착했는지 확인
        if src.len() < total_size {
            // 버퍼 예약 (성능 최적화)
            src.reserve(total_size - src.len());
            return Ok(None);
        }

        // 헤더 consume
        src.advance(HEADER_SIZE);

        // 페이로드 추출
        let payload = src.split_to(header.length as usize).freeze();

        // 프레임 디코딩 (압축 해제 포함)
        match self.decode_frame(header, payload) {
            Ok(frame) => Ok(Some(frame)),
            Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        }
    }
}

impl Encoder<Frame> for FrameCodec {
    type Error = io::Error;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // 최대 크기 체크
        if item.total_size() > self.config.max_frame_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Frame too large: {} bytes (max: {})",
                    item.total_size(),
                    self.config.max_frame_size
                ),
            ));
        }

        match self.encode_frame(&item) {
            Ok(encoded) => {
                dst.extend_from_slice(&encoded);
                Ok(())
            }
            Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        }
    }
}

/// 프레임 인코더 (단독 사용)
#[derive(Debug, Clone)]
pub struct FrameEncoder {
    config: FrameConfig,
    compressor: Option<Compressor>,
}

impl Default for FrameEncoder {
    fn default() -> Self {
        Self::new(FrameConfig::default())
    }
}

impl FrameEncoder {
    /// 새 인코더 생성
    pub fn new(config: FrameConfig) -> Self {
        let compressor = if config.compression_enabled {
            Some(
                Compressor::new(config.compression_level)
                    .with_threshold(config.compression_threshold),
            )
        } else {
            None
        };

        Self { config, compressor }
    }

    /// 압축 없이 생성
    pub fn without_compression() -> Self {
        Self::new(FrameConfig {
            compression_enabled: false,
            ..Default::default()
        })
    }

    /// 프레임 인코딩
    pub fn encode(&self, frame: &Frame) -> Result<BytesMut, FrameError> {
        // 최대 크기 체크
        if frame.total_size() > self.config.max_frame_size {
            return Err(FrameError::TooLarge {
                size: frame.total_size(),
                max: self.config.max_frame_size,
            });
        }

        let (payload, compressed) = if let Some(ref compressor) = self.compressor {
            if frame.frame_type() == FrameType::Data && frame.payload_len() > 0 {
                if let Some(compressed) = compressor.compress(&frame.payload) {
                    (compressed, true)
                } else {
                    (frame.payload.clone(), false)
                }
            } else {
                (frame.payload.clone(), false)
            }
        } else {
            (frame.payload.clone(), false)
        };

        let mut header = frame.header;
        header.length = payload.len() as u32;
        if compressed {
            header.flags |= FrameFlags::COMPRESSED;
        }

        let mut buf = BytesMut::with_capacity(HEADER_SIZE + payload.len());
        header.encode(&mut buf);
        buf.put_slice(&payload);

        Ok(buf)
    }

    /// 여러 프레임 인코딩
    pub fn encode_batch(&self, frames: &[Frame]) -> Result<BytesMut, FrameError> {
        let total_size: usize = frames.iter().map(|f| f.total_size()).sum();
        let mut buf = BytesMut::with_capacity(total_size);

        for frame in frames {
            let encoded = self.encode(frame)?;
            buf.extend_from_slice(&encoded);
        }

        Ok(buf)
    }

    /// 설정 조회
    pub fn config(&self) -> &FrameConfig {
        &self.config
    }
}

/// 프레임 디코더 (단독 사용)
#[derive(Debug, Clone)]
pub struct FrameDecoder {
    config: FrameConfig,
    decompressor: Decompressor,
    buffer: BytesMut,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new(FrameConfig::default())
    }
}

impl FrameDecoder {
    /// 새 디코더 생성
    pub fn new(config: FrameConfig) -> Self {
        Self {
            decompressor: Decompressor::new().with_max_size(config.max_frame_size),
            buffer: BytesMut::with_capacity(config.max_frame_size),
            config,
        }
    }

    /// 압축 없이 생성
    pub fn without_compression() -> Self {
        Self::new(FrameConfig {
            compression_enabled: false,
            ..Default::default()
        })
    }

    /// 데이터 추가
    pub fn push(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// 프레임 디코딩 시도
    pub fn decode(&mut self) -> Result<Option<Frame>, FrameError> {
        if self.buffer.len() < HEADER_SIZE {
            return Ok(None);
        }

        // 헤더 미리보기
        let header = {
            let mut peek = self.buffer.clone();
            FrameHeader::decode(&mut peek)?
        };

        let total_size = HEADER_SIZE + header.length as usize;

        // 최대 크기 체크
        if total_size > self.config.max_frame_size {
            return Err(FrameError::TooLarge {
                size: total_size,
                max: self.config.max_frame_size,
            });
        }

        // 전체 프레임 도착 확인
        if self.buffer.len() < total_size {
            return Ok(None);
        }

        // 헤더 consume
        self.buffer.advance(HEADER_SIZE);

        // 페이로드 추출
        let payload = self.buffer.split_to(header.length as usize).freeze();

        // 압축 해제
        let payload = if header.is_compressed() {
            self.decompressor.decompress(&payload)?
        } else {
            payload
        };

        Ok(Some(Frame {
            header: FrameHeader {
                frame_type: header.frame_type,
                flags: header.flags & !FrameFlags::COMPRESSED,
                length: payload.len() as u32,
                stream_id: header.stream_id,
            },
            payload,
        }))
    }

    /// 모든 프레임 디코딩
    pub fn decode_all(&mut self) -> Result<Vec<Frame>, FrameError> {
        let mut frames = Vec::new();
        while let Some(frame) = self.decode()? {
            frames.push(frame);
        }
        Ok(frames)
    }

    /// 버퍼 크기 조회
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    /// 버퍼 비우기
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// 설정 조회
    pub fn config(&self) -> &FrameConfig {
        &self.config
    }
}

/// 간단한 프레임 인코딩 (압축 없음)
pub fn encode_frame(frame: &Frame) -> BytesMut {
    frame.encode()
}

/// 간단한 프레임 디코딩 (압축 없음)
pub fn decode_frame(buf: &mut BytesMut, max_size: usize) -> Result<Option<Frame>, FrameError> {
    Frame::decode(buf, max_size)
}
