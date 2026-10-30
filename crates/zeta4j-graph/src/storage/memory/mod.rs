//! 메모리 관리 계층
//!
//! 메모리 풀, 메모리 제한, 페이지 교체 정책(LRU/ARC)을 제공합니다.
//!
//! ## 주요 구성요소
//!
//! - [`MemoryPool`]: 객체 풀링 (재사용으로 할당 비용 절감)
//! - [`MemoryLimiter`]: 메모리 사용량 제한
//! - [`LRUReplacer`]: LRU 페이지 교체 정책
//! - [`ARCReplacer`]: ARC(Adaptive Replacement Cache) 정책
//! - [`PinTracker`]: 트랜잭션별 pin 추적 (장기 TX 문제 해결)
//! - [`BackgroundFlusher`]: 비동기 dirty 페이지 flush

pub mod arc;
pub mod background_flusher;
pub mod limiter;
pub mod lru;
pub mod pin_tracker;
pub mod pool;

pub use arc::{ARCReplacer, ARCStats};
pub use background_flusher::{BackgroundFlusher, DirtyPageProvider, FlushConfig, FlushStats};
pub use limiter::{
    MemoryCategory, MemoryCategoryStats, MemoryLimitError, MemoryLimiter, MemoryLimiterConfig,
    MemoryReservation, MemoryStats, MemoryStatus,
};
pub use lru::LRUReplacer;
pub use pin_tracker::{PinTracker, PinTrackerStats};
pub use pool::{MemoryPool, MemoryPoolManager, PoolGuard, PoolStats, Poolable};
