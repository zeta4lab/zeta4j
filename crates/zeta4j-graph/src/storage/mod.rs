//! 그래프 스토리지 시스템
//!
//! ## 계층 구조
//!
//! | 계층 | 역할 | 주요 타입 |
//! |------|------|-----------|
//! | [`backend`] | 저장소 추상화 | NodeStorageBackend, EdgeStorageBackend |
//! | [`disk`] | 디스크 I/O | DiskManager, Page, SlottedPage |
//! | [`memory`] | 메모리 관리 | MemoryPool, MemoryLimiter, LRUReplacer |
//! | [`cache`] | 버퍼 캐시 | BufferPoolManager, PageWAL |
//! | [`graph`] | 그래프 저장 | NodeStorage, EdgeStorage |
//! | [`index`] | 인덱스 | BPlusTree |
//! | [`util`] | 유틸리티 | compression, config, lock helpers |

pub mod backend;
pub mod cache;
pub mod disk;
pub mod graph;
pub mod index;
pub mod memory;
pub mod util;

// Re-exports for convenience
pub use backend::{
    EdgeStorageBackend, LabelIndex, NodeStorageBackend, PropertyStore, TransactionalEdgeStorage,
    TransactionalNodeStorage,
};
