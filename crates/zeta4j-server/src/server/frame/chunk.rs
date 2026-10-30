//! # Chunked Transfer Module
//!
//! 대용량 데이터의 청크 단위 전송을 지원합니다.
//!
//! ## Features
//! - 스트리밍 청크 전송
//! - 청크 재조립
//! - 진행률 추적
//! - 체크섬 검증

#![allow(dead_code)]

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::{Frame, FrameError, FrameFlags, FrameType};

/// 청크 설정
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// 청크 크기 (기본: 64KB)
    pub chunk_size: usize,
    /// 최대 청크 수
    pub max_chunks: usize,
    /// 체크섬 활성화
    pub checksum_enabled: bool,
    /// 버퍼 크기
    pub buffer_size: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            chunk_size: 64 * 1024, // 64KB
            max_chunks: 1024,      // 최대 1024개 청크
            checksum_enabled: true,
            buffer_size: 256 * 1024, // 256KB 버퍼
        }
    }
}

impl ChunkConfig {
    /// 빌더 생성
    pub fn builder() -> ChunkConfigBuilder {
        ChunkConfigBuilder::default()
    }

    /// 청크 크기로 최대 전송 가능 크기 계산
    pub fn max_transfer_size(&self) -> usize {
        self.chunk_size * self.max_chunks
    }
}

/// 청크 설정 빌더
#[derive(Debug, Default)]
pub struct ChunkConfigBuilder {
    config: ChunkConfig,
}

impl ChunkConfigBuilder {
    /// 청크 크기 설정
    pub fn chunk_size(mut self, size: usize) -> Self {
        self.config.chunk_size = size;
        self
    }

    /// 최대 청크 수 설정
    pub fn max_chunks(mut self, max: usize) -> Self {
        self.config.max_chunks = max;
        self
    }

    /// 체크섬 활성화 설정
    pub fn checksum_enabled(mut self, enabled: bool) -> Self {
        self.config.checksum_enabled = enabled;
        self
    }

    /// 버퍼 크기 설정
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.config.buffer_size = size;
        self
    }

    /// 빌드
    pub fn build(self) -> ChunkConfig {
        self.config
    }
}

/// 청크 메타데이터
#[derive(Debug, Clone)]
pub struct ChunkMetadata {
    /// 전체 크기
    pub total_size: u64,
    /// 청크 수
    pub chunk_count: u32,
    /// 청크 크기
    pub chunk_size: u32,
    /// 전체 체크섬 (CRC32)
    pub checksum: Option<u32>,
    /// 스트림 ID
    pub stream_id: u16,
    /// 컨텐츠 타입 (옵션)
    pub content_type: Option<String>,
}

impl ChunkMetadata {
    /// 새 메타데이터 생성
    pub fn new(total_size: u64, chunk_size: usize) -> Self {
        let chunk_count = (total_size as usize).div_ceil(chunk_size) as u32;
        Self {
            total_size,
            chunk_count,
            chunk_size: chunk_size as u32,
            checksum: None,
            stream_id: 0,
            content_type: None,
        }
    }

    /// 체크섬 설정
    pub fn with_checksum(mut self, checksum: u32) -> Self {
        self.checksum = Some(checksum);
        self
    }

    /// 스트림 ID 설정
    pub fn with_stream_id(mut self, stream_id: u16) -> Self {
        self.stream_id = stream_id;
        self
    }

    /// 컨텐츠 타입 설정
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// 바이트로 인코딩
    pub fn encode(&self) -> Bytes {
        let content_type_bytes = self
            .content_type
            .as_ref()
            .map(|s| s.as_bytes())
            .unwrap_or(&[]);
        let mut buf = BytesMut::with_capacity(20 + content_type_bytes.len());

        buf.put_u64(self.total_size);
        buf.put_u32(self.chunk_count);
        buf.put_u32(self.chunk_size);
        buf.put_u32(self.checksum.unwrap_or(0));
        buf.put_u8(content_type_bytes.len() as u8);
        buf.put_slice(content_type_bytes);

        buf.freeze()
    }

    /// 바이트에서 디코딩
    pub fn decode(mut buf: impl Buf) -> Result<Self, FrameError> {
        if buf.remaining() < 21 {
            return Err(FrameError::Incomplete);
        }

        let total_size = buf.get_u64();
        let chunk_count = buf.get_u32();
        let chunk_size = buf.get_u32();
        let checksum_value = buf.get_u32();
        let checksum = if checksum_value != 0 {
            Some(checksum_value)
        } else {
            None
        };

        let content_type_len = buf.get_u8() as usize;
        let content_type = if content_type_len > 0 && buf.remaining() >= content_type_len {
            let mut content_buf = vec![0u8; content_type_len];
            buf.copy_to_slice(&mut content_buf);
            Some(String::from_utf8_lossy(&content_buf).to_string())
        } else {
            None
        };

        Ok(Self {
            total_size,
            chunk_count,
            chunk_size,
            checksum,
            stream_id: 0,
            content_type,
        })
    }
}

/// 청크 라이터 (데이터를 청크로 분할하여 전송)
pub struct ChunkedWriter {
    config: ChunkConfig,
    stream_id: u16,
    buffer: BytesMut,
    chunk_index: u32,
    total_written: u64,
    checksum: crc32fast::Hasher,
    started: bool,
    finished: bool,
}

impl ChunkedWriter {
    /// 새 청크 라이터 생성
    pub fn new(config: ChunkConfig, stream_id: u16) -> Self {
        Self {
            buffer: BytesMut::with_capacity(config.buffer_size),
            config,
            stream_id,
            chunk_index: 0,
            total_written: 0,
            checksum: crc32fast::Hasher::new(),
            started: false,
            finished: false,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults(stream_id: u16) -> Self {
        Self::new(ChunkConfig::default(), stream_id)
    }

    /// 데이터 추가
    pub fn write(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        if self.config.checksum_enabled {
            self.checksum.update(data);
        }
        self.total_written += data.len() as u64;
    }

    /// 청크 시작 프레임 생성
    pub fn start_frame(&mut self) -> Frame {
        self.started = true;

        let metadata = ChunkMetadata::new(self.total_written, self.config.chunk_size)
            .with_stream_id(self.stream_id);

        Frame::new(FrameType::ChunkStart, metadata.encode()).with_stream_id(self.stream_id)
    }

    /// 다음 청크 프레임 생성
    pub fn next_chunk(&mut self) -> Option<Frame> {
        if !self.started || self.finished {
            return None;
        }

        if self.buffer.is_empty() {
            return None;
        }

        let chunk_size = self.config.chunk_size.min(self.buffer.len());
        let chunk_data = self.buffer.split_to(chunk_size).freeze();

        // 청크 인덱스 포함
        let mut payload = BytesMut::with_capacity(4 + chunk_data.len());
        payload.put_u32(self.chunk_index);
        payload.put_slice(&chunk_data);

        self.chunk_index += 1;

        Some(Frame::new(FrameType::ChunkData, payload.freeze()).with_stream_id(self.stream_id))
    }

    /// 종료 프레임 생성
    pub fn finish_frame(&mut self) -> Frame {
        self.finished = true;

        let checksum = if self.config.checksum_enabled {
            std::mem::take(&mut self.checksum).finalize()
        } else {
            0
        };

        let mut payload = BytesMut::with_capacity(12);
        payload.put_u32(self.chunk_index); // 총 청크 수
        payload.put_u64(self.total_written); // 총 바이트 수
        // 체크섬은 플래그로 전달

        let mut frame = Frame::new(FrameType::ChunkEnd, payload.freeze())
            .with_stream_id(self.stream_id)
            .with_flags(FrameFlags::END_STREAM);

        // 체크섬이 있으면 ACK 플래그 사용
        if checksum != 0 {
            frame.header.flags |= FrameFlags::ACK;
        }

        frame
    }

    /// 모든 프레임 생성 (Iterator)
    pub fn into_frames(mut self) -> impl Iterator<Item = Frame> {
        let mut frames = Vec::new();

        // 시작 프레임
        frames.push(self.start_frame());

        // 청크 프레임들
        while let Some(chunk) = self.next_chunk() {
            frames.push(chunk);
        }

        // 종료 프레임
        frames.push(self.finish_frame());

        frames.into_iter()
    }

    /// 남은 버퍼 크기
    pub fn remaining(&self) -> usize {
        self.buffer.len()
    }

    /// 전송된 총 바이트
    pub fn total_written(&self) -> u64 {
        self.total_written
    }

    /// 청크 인덱스
    pub fn chunk_index(&self) -> u32 {
        self.chunk_index
    }

    /// 완료 여부
    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

/// 청크 리더 (청크들을 재조립하여 원본 데이터 복원)
pub struct ChunkedReader {
    config: ChunkConfig,
    metadata: Option<ChunkMetadata>,
    chunks: VecDeque<(u32, Bytes)>,
    received_chunks: u32,
    total_received: u64,
    checksum: crc32fast::Hasher,
    completed: bool,
    error: Option<FrameError>,
}

impl ChunkedReader {
    /// 새 청크 리더 생성
    pub fn new(config: ChunkConfig) -> Self {
        Self {
            config,
            metadata: None,
            chunks: VecDeque::new(),
            received_chunks: 0,
            total_received: 0,
            checksum: crc32fast::Hasher::new(),
            completed: false,
            error: None,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(ChunkConfig::default())
    }

    /// 프레임 처리
    pub fn process_frame(&mut self, frame: &Frame) -> Result<(), FrameError> {
        match frame.frame_type() {
            FrameType::ChunkStart => self.handle_start(frame),
            FrameType::ChunkData => self.handle_data(frame),
            FrameType::ChunkEnd => self.handle_end(frame),
            _ => Err(FrameError::Protocol(format!(
                "Unexpected frame type: {}",
                frame.frame_type()
            ))),
        }
    }

    fn handle_start(&mut self, frame: &Frame) -> Result<(), FrameError> {
        if self.metadata.is_some() {
            return Err(FrameError::Protocol("Duplicate ChunkStart".into()));
        }

        let metadata = ChunkMetadata::decode(&frame.payload[..])?;

        // 최대 크기 체크
        let max_size = self.config.max_transfer_size();
        if metadata.total_size > max_size as u64 {
            return Err(FrameError::TooLarge {
                size: metadata.total_size as usize,
                max: max_size,
            });
        }

        self.metadata = Some(metadata);
        Ok(())
    }

    fn handle_data(&mut self, frame: &Frame) -> Result<(), FrameError> {
        if self.metadata.is_none() {
            return Err(FrameError::Protocol("ChunkData before ChunkStart".into()));
        }

        if frame.payload.len() < 4 {
            return Err(FrameError::Incomplete);
        }

        let mut payload = frame.payload.clone();
        let chunk_index = payload.get_u32();
        let chunk_data = payload;

        // 체크섬 업데이트
        if self.config.checksum_enabled {
            self.checksum.update(&chunk_data);
        }

        self.total_received += chunk_data.len() as u64;
        self.received_chunks += 1;
        self.chunks.push_back((chunk_index, chunk_data));

        // 최대 청크 수 체크
        if self.received_chunks > self.config.max_chunks as u32 {
            return Err(FrameError::TooLarge {
                size: self.received_chunks as usize,
                max: self.config.max_chunks,
            });
        }

        Ok(())
    }

    fn handle_end(&mut self, frame: &Frame) -> Result<(), FrameError> {
        if self.metadata.is_none() {
            return Err(FrameError::Protocol("ChunkEnd before ChunkStart".into()));
        }

        let mut payload = frame.payload.clone();
        if payload.remaining() < 12 {
            return Err(FrameError::Incomplete);
        }

        let total_chunks = payload.get_u32();
        let total_bytes = payload.get_u64();

        // 검증
        if self.received_chunks != total_chunks {
            return Err(FrameError::Protocol(format!(
                "Chunk count mismatch: expected {}, got {}",
                total_chunks, self.received_chunks
            )));
        }

        if self.total_received != total_bytes {
            return Err(FrameError::Protocol(format!(
                "Byte count mismatch: expected {}, got {}",
                total_bytes, self.total_received
            )));
        }

        self.completed = true;
        Ok(())
    }

    /// 완료 여부
    pub fn is_completed(&self) -> bool {
        self.completed
    }

    /// 데이터 재조립
    pub fn assemble(&mut self) -> Result<Bytes, FrameError> {
        if !self.completed {
            return Err(FrameError::Incomplete);
        }

        // 인덱스 순으로 정렬
        let mut sorted: Vec<_> = self.chunks.drain(..).collect();
        sorted.sort_by_key(|(idx, _)| *idx);

        // 데이터 병합
        let total_size = self
            .metadata
            .as_ref()
            .map(|m| m.total_size as usize)
            .unwrap_or(0);
        let mut result = BytesMut::with_capacity(total_size);

        for (_, data) in sorted {
            result.extend_from_slice(&data);
        }

        Ok(result.freeze())
    }

    /// 메타데이터 조회
    pub fn metadata(&self) -> Option<&ChunkMetadata> {
        self.metadata.as_ref()
    }

    /// 진행률 (0.0 ~ 1.0)
    pub fn progress(&self) -> f64 {
        match &self.metadata {
            Some(meta) if meta.total_size > 0 => {
                self.total_received as f64 / meta.total_size as f64
            }
            _ => 0.0,
        }
    }

    /// 수신된 청크 수
    pub fn received_chunks(&self) -> u32 {
        self.received_chunks
    }

    /// 수신된 총 바이트
    pub fn total_received(&self) -> u64 {
        self.total_received
    }

    /// 리셋
    pub fn reset(&mut self) {
        self.metadata = None;
        self.chunks.clear();
        self.received_chunks = 0;
        self.total_received = 0;
        self.checksum = crc32fast::Hasher::new();
        self.completed = false;
        self.error = None;
    }
}

/// 청크 진행률 추적
#[derive(Debug)]
pub struct ChunkProgress {
    /// 전체 크기
    pub total_size: AtomicU64,
    /// 전송된/수신된 크기
    pub transferred: AtomicU64,
    /// 청크 수
    pub chunk_count: AtomicUsize,
    /// 완료된 청크 수
    pub completed_chunks: AtomicUsize,
}

impl Default for ChunkProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkProgress {
    /// 새 진행률 추적 생성
    pub fn new() -> Self {
        Self {
            total_size: AtomicU64::new(0),
            transferred: AtomicU64::new(0),
            chunk_count: AtomicUsize::new(0),
            completed_chunks: AtomicUsize::new(0),
        }
    }

    /// 전체 크기 설정
    pub fn set_total(&self, total: u64, chunks: usize) {
        self.total_size.store(total, Ordering::Relaxed);
        self.chunk_count.store(chunks, Ordering::Relaxed);
    }

    /// 전송/수신 기록
    pub fn record(&self, bytes: u64) {
        self.transferred.fetch_add(bytes, Ordering::Relaxed);
        self.completed_chunks.fetch_add(1, Ordering::Relaxed);
    }

    /// 진행률 (0.0 ~ 1.0)
    pub fn progress(&self) -> f64 {
        let total = self.total_size.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let transferred = self.transferred.load(Ordering::Relaxed);
        (transferred as f64 / total as f64).min(1.0)
    }

    /// 진행률 (퍼센트)
    pub fn progress_percent(&self) -> u8 {
        (self.progress() * 100.0) as u8
    }

    /// 완료 여부
    pub fn is_completed(&self) -> bool {
        let total = self.chunk_count.load(Ordering::Relaxed);
        let completed = self.completed_chunks.load(Ordering::Relaxed);
        total > 0 && completed >= total
    }

    /// 리셋
    pub fn reset(&self) {
        self.total_size.store(0, Ordering::Relaxed);
        self.transferred.store(0, Ordering::Relaxed);
        self.chunk_count.store(0, Ordering::Relaxed);
        self.completed_chunks.store(0, Ordering::Relaxed);
    }
}

/// CRC32 체크섬 계산
pub fn calculate_checksum(data: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// 체크섬 검증
pub fn verify_checksum(data: &[u8], expected: u32) -> bool {
    calculate_checksum(data) == expected
}
