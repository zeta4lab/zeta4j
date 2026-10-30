//! # Zeta4j Vector
//!
//! canonical vector record에서 파생되는 immutable HNSW generation과 검색 모듈입니다.
//!
//! ## Quick Start
//!
//! ```ignore
//! use zeta4j_vector::prelude::*;
//!
//! let generation = HnswGeneration::build(
//!     vec![(1, vec![0.1; 128]), (2, vec![0.2; 128])],
//!     HnswGenerationParams::new(128, DistanceFunction::Cosine),
//! )?;
//! let results = generation.search(&vec![0.15; 128], 10, None)?;
//! ```
//!
//! ## API Tiers
//!
//! | Tier | Access | Description |
//! |------|--------|-------------|
//! | **prelude** | `use zeta4j_vector::prelude::*` | Core types for everyday use |
//! | **root** | `use zeta4j_vector::*` | Commonly used types |
//! | **module** | `use zeta4j_vector::index::*` | Advanced features |
//!
//! ## Module Structure
//!
//! ### Core Layer
//! - [`index`] - immutable HNSW generation과 거리 함수
//! - [`search`] - Vector/Fulltext/Graph 후보 병합

// =============================================================================
// Module Structure
// =============================================================================

pub mod index;
pub mod prelude;
pub mod search;

// =============================================================================
// Public API - Index Types (Core)
// =============================================================================

pub use index::{
    DistanceFunction, HNSWError, HnswGeneration, HnswGenerationNode, HnswGenerationParams,
    SearchResult,
};

// =============================================================================
// Public API - Search Types
// =============================================================================

pub use search::{HybridMerger, HybridQueryConfig, HybridResult, MergeMode};
