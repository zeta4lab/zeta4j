//! 쿼리 캐시 시스템
//!
//! 쿼리 실행 성능을 향상시키기 위한 다단계 캐시 시스템을 제공합니다.
//!
//! # 캐시 계층
//!
//! 1. **ParseCache**: 쿼리 문자열 → AST 매핑
//! 2. **PlanCache**: AST + 파라미터 → 실행 계획 매핑
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use crate::cache::{QueryCacheManager, CacheConfig};
//!
//! // 캐시 관리자 생성
//! let mut manager = QueryCacheManager::with_defaults();
//!
//! // 파싱 캐시 활용
//! let ast = manager.get_or_parse("MATCH (n) RETURN n", |query| {
//!     parser.parse(query)
//! })?;
//!
//! // 실행 계획 캐시 활용
//! let plan = manager.get_or_plan("MATCH (n) RETURN n", &[], |query, params| {
//!     planner.create_plan(query, params)
//! })?;
//!
//! // 통계 확인
//! let stats = manager.stats();
//! println!("Hit ratio: {:.2}%", stats.total_hit_ratio() * 100.0);
//! ```
//!
//! # 무효화
//!
//! 스키마 변경, 인덱스 변경, 통계 변경 시 자동으로 캐시를 무효화합니다.
//!
//! ```rust,ignore
//! // 스키마 변경 시
//! manager.on_schema_change();
//!
//! // 인덱스 변경 시
//! manager.on_index_change("Person", "name");
//!
//! // 통계 변경 시
//! manager.on_stats_change(vec!["Person".to_string()]);
//! ```

mod concurrent_cache;
mod concurrent_manager;
mod config;
mod entry;
mod invalidation;
mod key;
mod manager;
mod parse_cache;
mod plan_cache;
mod stats;

pub use concurrent_cache::{CacheStatsSnapshot, ConcurrentCacheStats, ConcurrentLruCache};
pub use concurrent_manager::{ConcurrentQueryCacheManager, QueryCacheStats};
pub use config::CacheConfig;
pub use entry::CacheEntry;
pub use invalidation::{InvalidationEvent, InvalidationResult};
pub use key::QueryCacheKey;
pub use manager::QueryCacheManager;
pub use parse_cache::ParseCache;
pub use plan_cache::PlanCache;
pub use stats::{CacheStats, CombinedCacheStats};
