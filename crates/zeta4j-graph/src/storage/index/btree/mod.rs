//! B+ Tree 인덱스 구현
//!
//! 이 모듈은 B+ Tree 인덱스를 제공합니다.
//! - `index`: 핵심 구조체 및 유틸리티
//! - `search`: 값 조회 및 범위 검색
//! - `insert`: 삽입 및 페이지 분할
//! - `delete`: 삭제 연산
//! - `rebalance`: Underflow 처리 및 페이지 재조정
//! - `latch`: Crabbing Protocol을 위한 페이지 래치 관리
//! - `concurrent`: Concurrent B+ Tree (Crabbing Protocol 적용)
//! - `bulk_loader`: 대량 데이터 로딩 (Bulk Loading)

pub mod bulk_loader;
pub mod concurrent;
pub mod index;
pub mod internal_page;
pub mod latch;
pub mod leaf_page;
pub mod metadata;
pub mod multivalue;
pub mod slotted;
pub mod types;

// 연산 모듈 (index.rs의 impl BPlusTree를 확장)
mod delete;
mod insert;
mod rebalance;
mod search;

// Phase 5: Online Rebuild 관련 타입 re-export
pub use index::{LeafChainReport, LeafPageInfo, RebuildResult, VerifyResult};

// Multi-value B+Tree re-export
pub use multivalue::MultiValueBPlusTree;

// Core B+ Tree re-export
pub use index::BPlusTree;
