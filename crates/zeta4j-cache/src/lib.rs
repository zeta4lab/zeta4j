// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Zeta4j Cache - Two-tier Caching System
//!
//! 2-tier 캐시 시스템을 제공합니다:
//! - **L1 (moka)**: 고성능 인메모리 LRU 캐시
//! - **L2 (redb)**: 영구 저장소, 서버 재시작 후에도 유지
//!
//! # 사용법
//!
//! ```ignore
//! use zeta4j_cache::{TieredCache, CacheConfig};
//!
//! // 캐시 생성
//! let config = CacheConfig::default();
//! let cache: TieredCache<String, bool> = TieredCache::new(config, "/tmp/cache")?;
//!
//! // 값 조회 또는 생성
//! let value = cache.get_or_insert("key".to_string(), || expensive_compute());
//!
//! // 값 무효화
//! cache.invalidate(&"key".to_string());
//! ```
//!
//! # IndexCache
//!
//! 인덱스 메타데이터 캐싱에 특화된 구현입니다.
//!
//! ```ignore
//! use zeta4j_cache::IndexCache;
//!
//! let db = std::sync::Arc::new(redb::Database::create("/tmp/index-cache.redb")?);
//! let cache = IndexCache::new(Some(db))?;
//!
//! // 인덱스 존재 여부 확인 (캐시 히트 시 빠른 응답)
//! let exists = cache.has_index("Person", "name", || {
//!     engine.has_property_index("Person", "name")
//! });
//! ```

mod config;
mod error;
mod index_cache;
mod l1_moka;
mod l2_redb;
mod stats;
mod tiered;

pub mod prelude;

pub use config::CacheConfig;
pub use error::{Result, ZetaError};
pub use index_cache::IndexCache;
pub use l1_moka::{L1Cache, SharedL1Cache};
pub use stats::CacheStats;
pub use tiered::TieredCache;
