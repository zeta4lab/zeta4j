//! Streaming Response Module (M2.4)
//!
//! 대용량 쿼리 결과의 효율적인 전송을 위한 스트리밍 응답
//!
//! ## Features
//! - JSON Lines (ndjson) 스트리밍
//! - Server-Sent Events (SSE) 지원
//! - 백프레셔 및 취소 지원
//! - Long Polling 지원

pub mod backpressure;
pub mod executor;
pub mod jsonl;
pub mod poll;
pub mod sse;
pub mod types;

// Public re-exports
pub use types::{
    JsonLinesRecord, PollProgress, PollResponse, PollStatus, RecordMeta, RecoveryStrategy,
    SseEventType, StreamError, StreamFooter, StreamFormat, StreamHeader, StreamStats,
    StreamingConfig, StreamingQueryRequest,
};

pub use backpressure::{
    BackpressureError, BufferState, SharedBackpressure, StreamBackpressure, StreamPermit,
    create_backpressure,
};
pub use executor::{
    CancellableStreamingResult, SharedStreamingExecutor, StreamingExecutor, StreamingResult,
    create_streaming_executor,
};
pub use jsonl::{JsonLinesFormatter, JsonLinesStream, JsonLinesStreamBuilder};
pub use poll::{
    PollError, PollManager, PollManagerConfig, QueryState, SharedPollManager, create_poll_manager,
};
pub use sse::{SseEventBuilder, SseStream, SseStreamBuilder};
