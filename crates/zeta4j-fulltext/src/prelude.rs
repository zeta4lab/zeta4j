// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Prelude module for convenient imports.
//!
//! This module re-exports the most commonly used types from the crate.
//!
//! # Example
//!
//! ```rust,ignore
//! use zeta4j_fulltext::prelude::*;
//!
//! // Create fulltext index
//! let config = FulltextConfig::default();
//! let index = FulltextIndex::create("/path/to/index", config)?;
//!
//! // Add document and search
//! index.add_document(1, "Hello, world!")?;
//! let results = index.search_bm25("hello", 10)?;
//! ```

// =============================================================================
// Core Types
// =============================================================================

pub use crate::error::{FulltextError, Result};
pub use crate::index::{FulltextConfig, FulltextIndex};

// =============================================================================
// Search
// =============================================================================

pub use crate::search::bm25::SearchResult;

// =============================================================================
// Writer
// =============================================================================

pub use crate::index::writer::ChunkDocument;
