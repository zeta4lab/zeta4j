// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # zeta4j-fulltext
//!
//! Tantivy 기반 전문 검색 엔진으로, 한국어를 포함한 다국어 지원을 제공합니다.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use zeta4j_fulltext::prelude::*;
//!
//! // Create fulltext index
//! let config = FulltextConfig::default();
//! let index = FulltextIndex::create("/path/to/index", config)?;
//!
//! // Add document and search
//! index.add_document(1, "데이터베이스 인덱싱 성능 최적화")?;
//! let results = index.search_bm25("인덱싱", 10)?;
//! ```
//!
//! ## API Tiers
//!
//! | Tier | Access | Description |
//! |------|--------|-------------|
//! | **prelude** | `use zeta4j_fulltext::prelude::*` | Core types for everyday use |
//! | **root** | `use zeta4j_fulltext::*` | Commonly used types |
//! | **module** | `use zeta4j_fulltext::tokenizer::*` | Advanced features |
//!
//! ## Module Structure
//!
//! ### Index Layer
//! - [`index`] - Fulltext index management
//!   - `FulltextIndex` - Main index interface
//!   - `FulltextConfig` - Index configuration
//!   - `IndexWriter` / `IndexReader` - Low-level access
//!
//! ### Search Layer
//! - [`search`] - Search functionality
//!   - `Bm25Searcher` - BM25 ranking search
//!   - `SearchResult` - Search result type
//!
//! ### Tokenizer Layer
//! - [`tokenizer`] - Text tokenization
//!   - `LanguageRouter` - Multi-language tokenizer routing
//!
//! ### Storage Layer
//! - [`storage`] - Index persistence
//!   - `DiskStorage` - Disk-based index storage
//!
//! ## Features
//!
//! - **Tantivy BM25 Search**: High-performance fulltext search
//! - **Korean Morphological Analysis**: Lindera-based Korean tokenization
//! - **Multi-language Support**: Auto language detection and tokenizer routing
//! - **Disk Persistence**: Persistent index storage

// =============================================================================
// Module Structure
// =============================================================================

mod error;
pub mod index;
pub mod prelude;
pub mod search;
pub mod storage;
pub mod tokenizer;

// =============================================================================
// Public API - Core Types
// =============================================================================

pub use error::{FulltextError, Result};
pub use index::{FulltextConfig, FulltextIndex, IndexHealth, RetryConfig, calculate_heap_size};

// =============================================================================
// Public API - Index
// =============================================================================

pub use index::reader::{IndexReader, PaginationOptions};
pub use index::schema::ChunkSchema;
pub use index::writer::{ChunkDocument, IndexWriter};

// =============================================================================
// Public API - Search
// =============================================================================

pub use search::bm25::{Bm25Searcher, Highlight, SearchResult, TextHighlighter};

// =============================================================================
// Public API - Tokenizer
// =============================================================================

pub use tokenizer::router::{DetectedLanguage, LanguageRouter};

// =============================================================================
// Public API - Storage
// =============================================================================

pub use storage::disk::{ChunkMappings, DiskStorage};
pub use storage::redb_directory::{FulltextGcReport, FulltextGeneration, RedbDirectory};
