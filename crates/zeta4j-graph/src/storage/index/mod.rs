//! 인덱스 계층
//!
//! B+ 트리 등 스토리지 인덱스 구조를 제공합니다.
//!
//! ## 트레이트
//!
//! - [`Index`]: 기본 키-값 인덱스
//! - [`RangeIndex`]: 범위 검색 지원 인덱스
//! - [`MultiValueIndex`]: 멀티값 인덱스
//! - [`ConcurrentIndex`]: 동시성 안전 인덱스
//! - [`PersistentIndex`]: 영속성 인덱스

pub mod btree;
pub mod traits;

// Re-export traits
pub use traits::{
    ConcurrentIndex, Index, MultiValueIndex, PersistentIndex, RangeIndex,
    RebuildResult as IndexRebuildResult, VerifyResult as IndexVerifyResult,
};

// Re-export B+Tree
pub use btree::BPlusTree;
