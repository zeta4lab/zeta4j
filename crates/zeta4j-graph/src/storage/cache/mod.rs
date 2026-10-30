//! 버퍼 캐시 계층
//!
//! 디스크와 메모리 사이의 페이지 캐싱을 담당합니다.
//! WAL(Write-Ahead Logging), LSN(Log Sequence Number) 지원을 포함합니다.

mod lsn; // impl 확장 모듈 (내부용)
pub mod pool;
pub mod types;
mod wal; // impl 확장 모듈 (내부용)

pub use pool::BufferPoolManager;
pub use types::{BufferPoolStats, FrameId};

// PageId re-export (lsn.rs에서 사용)
pub use crate::storage::disk::page::PageId;
