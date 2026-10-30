//! Prelude module for convenient imports.
//!
//! This module re-exports the most commonly used types from the crate.
//!
//! # Example
//!
//! ```rust,ignore
//! use zeta4j_vector::prelude::*;
//!
//! let generation = HnswGeneration::build(
//!     vec![(1, vec![0.1; 128])],
//!     HnswGenerationParams::new(128, DistanceFunction::Cosine),
//! )?;
//! let results = generation.search(&vec![0.15; 128], 10, None)?;
//! ```

// =============================================================================
// Index Types (Core)
// =============================================================================

pub use crate::index::{DistanceFunction, HnswGeneration, HnswGenerationParams, SearchResult};

// =============================================================================
// Search Types
// =============================================================================

pub use crate::search::{HybridMerger, HybridResult, MergeMode};
