// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Streaming Types (M2.4)
//!
//! 스트리밍 응답을 위한 타입 정의

use serde::{Deserialize, Serialize};

/// 스트리밍 쿼리 요청
#[derive(Debug, Clone, Deserialize)]
pub struct StreamingQueryRequest {
    pub statement: String,
    #[serde(default)]
    pub parameters: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub batch_size: Option<usize>,
    #[serde(default)]
    pub format: StreamFormat,
}

impl Default for StreamingQueryRequest {
    fn default() -> Self {
        Self {
            statement: String::new(),
            parameters: serde_json::Map::new(),
            batch_size: None,
            format: StreamFormat::default(),
        }
    }
}

/// 스트리밍 포맷
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StreamFormat {
    #[default]
    JsonLines,
    Sse,
    Chunked,
}

/// 스트림 헤더
#[derive(Debug, Clone, Serialize)]
pub struct StreamHeader {
    pub columns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_rows: Option<u64>,
    pub query_id: String,
}

/// JSON Lines 레코드
#[derive(Debug, Clone, Serialize)]
pub struct JsonLinesRecord {
    #[serde(flatten)]
    pub data: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<RecordMeta>,
}

/// 레코드 메타데이터
#[derive(Debug, Clone, Serialize)]
pub struct RecordMeta {
    pub index: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// 스트림 푸터
#[derive(Debug, Clone, Serialize)]
pub struct StreamFooter {
    pub rows_sent: u64,
    pub took_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<StreamStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
}

/// 스트림 통계
#[derive(Debug, Clone, Serialize, Default)]
pub struct StreamStats {
    pub contains_updates: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodes_created: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodes_deleted: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationships_created: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationships_deleted: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties_set: Option<u64>,
}

/// SSE 이벤트 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SseEventType {
    Header,
    Row,
    Footer,
    Error,
    Heartbeat,
}

/// Poll 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PollStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Poll 진행 상황
#[derive(Debug, Clone, Serialize, Default)]
pub struct PollProgress {
    pub rows_fetched: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage: Option<f32>,
}

/// Poll 응답
#[derive(Debug, Clone, Serialize)]
pub struct PollResponse {
    pub query_id: String,
    pub status: PollStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_url: Option<String>,
    pub progress: PollProgress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 스트리밍 에러 (Phase 5: Error Handling)
#[derive(Debug, Clone, Serialize, thiserror::Error)]
pub enum StreamError {
    #[error("Query error: {0}")]
    #[serde(rename = "query_error")]
    QueryError(String),

    #[error("Connection closed")]
    #[serde(rename = "connection_closed")]
    ConnectionClosed,

    #[error("Stream timeout")]
    #[serde(rename = "timeout")]
    Timeout,

    #[error("Stream cancelled")]
    #[serde(rename = "cancelled")]
    Cancelled,

    #[error("Backpressure limit exceeded")]
    #[serde(rename = "backpressure")]
    BackpressureExceeded,

    #[error("Client disconnected")]
    #[serde(rename = "client_disconnected")]
    ClientDisconnected,

    #[error("Resource exhausted: {0}")]
    #[serde(rename = "resource_exhausted")]
    ResourceExhausted(String),

    #[error("Internal error: {0}")]
    #[serde(rename = "internal_error")]
    InternalError(String),
}

impl StreamError {
    /// 에러가 재시도 가능한지 여부
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            StreamError::Timeout
                | StreamError::BackpressureExceeded
                | StreamError::ResourceExhausted(_)
        )
    }

    /// 에러 코드
    pub fn code(&self) -> &'static str {
        match self {
            StreamError::QueryError(_) => "QUERY_ERROR",
            StreamError::ConnectionClosed => "CONNECTION_CLOSED",
            StreamError::Timeout => "TIMEOUT",
            StreamError::Cancelled => "CANCELLED",
            StreamError::BackpressureExceeded => "BACKPRESSURE",
            StreamError::ClientDisconnected => "CLIENT_DISCONNECTED",
            StreamError::ResourceExhausted(_) => "RESOURCE_EXHAUSTED",
            StreamError::InternalError(_) => "INTERNAL_ERROR",
        }
    }
}

/// 복구 전략 (Phase 5: Recovery Strategies)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// 자동 재시도
    Retry { max_attempts: u32, delay_ms: u64 },
    /// 부분 결과 반환
    PartialResult,
    /// 즉시 실패
    FailFast,
    /// 무시하고 계속
    IgnoreAndContinue,
}

impl Default for RecoveryStrategy {
    fn default() -> Self {
        RecoveryStrategy::Retry {
            max_attempts: 3,
            delay_ms: 100,
        }
    }
}

/// 스트리밍 설정 (Phase 4: Streaming Optimizations)
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// 기본 배치 크기
    pub default_batch_size: usize,
    /// 최대 배치 크기
    pub max_batch_size: usize,
    /// 버퍼 크기 (채널)
    pub buffer_size: usize,
    /// 청크 크기
    pub chunk_size: usize,
    /// 하트비트 간격 (ms)
    pub heartbeat_interval_ms: u64,
    /// 스트림 타임아웃 (ms)
    pub timeout_ms: u64,
    /// 프리페칭 활성화
    pub enable_prefetch: bool,
    /// 프리페치 버퍼 크기
    pub prefetch_buffer_size: usize,
    /// 압축 활성화
    pub enable_compression: bool,
    /// 압축 수준 (1-9)
    pub compression_level: u32,
    /// 캐싱 활성화
    pub enable_cache: bool,
    /// 캐시 TTL (ms)
    pub cache_ttl_ms: u64,
    /// 최대 캐시 항목 수
    pub max_cache_entries: usize,
    /// 복구 전략
    pub recovery_strategy: RecoveryStrategy,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            default_batch_size: 1000,
            max_batch_size: 10000,
            buffer_size: 1024,
            chunk_size: 64 * 1024,        // 64KB
            heartbeat_interval_ms: 15000, // 15초
            timeout_ms: 300000,           // 5분
            // Phase 4 optimizations
            enable_prefetch: true,
            prefetch_buffer_size: 100,
            enable_compression: false,
            compression_level: 6,
            enable_cache: false,
            cache_ttl_ms: 60000, // 1분
            max_cache_entries: 1000,
            recovery_strategy: RecoveryStrategy::default(),
        }
    }
}

impl StreamingConfig {
    /// 프리페치 최적화 설정
    pub fn with_prefetch(mut self, enable: bool, buffer_size: usize) -> Self {
        self.enable_prefetch = enable;
        self.prefetch_buffer_size = buffer_size;
        self
    }

    /// 압축 최적화 설정
    pub fn with_compression(mut self, enable: bool, level: u32) -> Self {
        self.enable_compression = enable;
        self.compression_level = level.clamp(1, 9);
        self
    }

    /// 캐싱 최적화 설정
    pub fn with_cache(mut self, enable: bool, ttl_ms: u64, max_entries: usize) -> Self {
        self.enable_cache = enable;
        self.cache_ttl_ms = ttl_ms;
        self.max_cache_entries = max_entries;
        self
    }

    /// 복구 전략 설정
    pub fn with_recovery(mut self, strategy: RecoveryStrategy) -> Self {
        self.recovery_strategy = strategy;
        self
    }

    /// 고성능 모드 설정
    pub fn high_performance() -> Self {
        Self {
            default_batch_size: 5000,
            max_batch_size: 50000,
            buffer_size: 4096,
            chunk_size: 256 * 1024,       // 256KB
            heartbeat_interval_ms: 30000, // 30초
            timeout_ms: 600000,           // 10분
            enable_prefetch: true,
            prefetch_buffer_size: 500,
            enable_compression: true,
            compression_level: 4,
            enable_cache: true,
            cache_ttl_ms: 300000, // 5분
            max_cache_entries: 10000,
            recovery_strategy: RecoveryStrategy::Retry {
                max_attempts: 5,
                delay_ms: 200,
            },
        }
    }

    /// 저지연 모드 설정
    pub fn low_latency() -> Self {
        Self {
            default_batch_size: 100,
            max_batch_size: 1000,
            buffer_size: 256,
            chunk_size: 16 * 1024,       // 16KB
            heartbeat_interval_ms: 5000, // 5초
            timeout_ms: 60000,           // 1분
            enable_prefetch: true,
            prefetch_buffer_size: 20,
            enable_compression: false,
            compression_level: 1,
            enable_cache: false,
            cache_ttl_ms: 30000,
            max_cache_entries: 100,
            recovery_strategy: RecoveryStrategy::FailFast,
        }
    }
}
