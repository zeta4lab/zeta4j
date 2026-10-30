//! 인덱스 메타데이터 캐시
//!
//! GraphEngine의 인덱스 조회를 캐싱하여 성능 향상

use crate::config::CacheConfig;
use crate::error::Result;
use crate::l1_moka::L1Cache;
use crate::l2_redb::L2Cache;
use crate::stats::CacheStats;
use std::collections::HashSet;
use std::sync::Arc;

/// 인덱스 캐시 키 (label, property)
pub type IndexKey = (String, String);

/// 인덱스 캐시
///
/// 인덱스 메타데이터를 캐싱하여 has_property_index 호출 성능 향상
pub struct IndexCache {
    /// 개별 인덱스 존재 여부 캐시 (label, property) -> bool
    exists_l1: L1Cache<IndexKey, bool>,
    exists_l2: Option<L2Cache<IndexKey, bool>>,

    /// 전체 인덱스 목록 캐시 (단일 키)
    all_indexes_l1: L1Cache<(), HashSet<IndexKey>>,
    all_indexes_l2: Option<L2Cache<(), HashSet<IndexKey>>>,

    /// 캐시 통계
    stats: Arc<CacheStats>,

    /// 설정
    config: CacheConfig,
}

impl IndexCache {
    /// 새 IndexCache 생성
    pub fn new(db: Option<Arc<redb::Database>>) -> Result<Self> {
        let config = CacheConfig::for_index_cache();

        let exists_l1 = L1Cache::new(config.l1_max_capacity, config.l1_ttl);
        let all_indexes_l1 = L1Cache::new(1, config.l1_ttl);

        let (exists_l2, all_indexes_l2) = if let Some(db) = db {
            let exists = L2Cache::with_db(db.clone(), "index_exists")?;
            let all = L2Cache::with_db(db, "index_all")?;
            (Some(exists), Some(all))
        } else {
            (None, None)
        };

        Ok(Self {
            exists_l1,
            exists_l2,
            all_indexes_l1,
            all_indexes_l2,
            stats: Arc::new(CacheStats::new()),
            config,
        })
    }

    /// L1 전용 IndexCache 생성
    pub fn l1_only() -> Self {
        let config = CacheConfig::for_index_cache();

        Self {
            exists_l1: L1Cache::new(config.l1_max_capacity, config.l1_ttl),
            exists_l2: None,
            all_indexes_l1: L1Cache::new(1, config.l1_ttl),
            all_indexes_l2: None,
            stats: Arc::new(CacheStats::new()),
            config,
        }
    }

    /// 인덱스 존재 여부 확인 (캐시 활용)
    ///
    /// 캐시 히트 시 빠른 응답, 미스 시 fetch 함수 호출
    pub fn has_index<F>(&self, label: &str, prop: &str, fetch: F) -> bool
    where
        F: FnOnce() -> bool,
    {
        let key = (label.to_string(), prop.to_string());

        // L1 체크
        if let Some(exists) = self.exists_l1.get(&key) {
            if self.config.stats_enabled {
                self.stats.record_l1_hit();
            }
            return exists;
        }

        // L2 체크
        if let Some(ref l2) = self.exists_l2 {
            match l2.get(&key) {
                Ok(Some(exists)) => {
                    if self.config.stats_enabled {
                        self.stats.record_l2_hit();
                    }
                    // L1에 승격
                    self.exists_l1.insert(key, exists);
                    return exists;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("IndexCache L2 get error: {}", e);
                }
            }
        }

        // 캐시 미스: fetch 실행
        if self.config.stats_enabled {
            self.stats.record_miss();
        }

        let exists = fetch();

        // 캐시에 저장
        self.exists_l1.insert(key.clone(), exists);
        if let Some(ref l2) = self.exists_l2
            && let Err(e) = l2.insert(&key, &exists)
        {
            tracing::warn!("IndexCache L2 insert error: {}", e);
        }

        if self.config.stats_enabled {
            self.stats.record_insert();
        }

        exists
    }

    /// 전체 인덱스 목록 조회 (캐시 활용)
    pub fn get_all<F>(&self, fetch: F) -> HashSet<IndexKey>
    where
        F: FnOnce() -> HashSet<IndexKey>,
    {
        let key = ();

        // L1 체크
        if let Some(indexes) = self.all_indexes_l1.get(&key) {
            if self.config.stats_enabled {
                self.stats.record_l1_hit();
            }
            return indexes;
        }

        // L2 체크
        if let Some(ref l2) = self.all_indexes_l2 {
            match l2.get(&key) {
                Ok(Some(indexes)) => {
                    if self.config.stats_enabled {
                        self.stats.record_l2_hit();
                    }
                    // L1에 승격
                    self.all_indexes_l1.insert(key, indexes.clone());
                    return indexes;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("IndexCache all_indexes L2 get error: {}", e);
                }
            }
        }

        // 캐시 미스: fetch 실행
        if self.config.stats_enabled {
            self.stats.record_miss();
        }

        let indexes = fetch();

        // 캐시에 저장
        self.all_indexes_l1.insert(key, indexes.clone());
        if let Some(ref l2) = self.all_indexes_l2
            && let Err(e) = l2.insert(&key, &indexes)
        {
            tracing::warn!("IndexCache all_indexes L2 insert error: {}", e);
        }

        if self.config.stats_enabled {
            self.stats.record_insert();
        }

        indexes
    }

    /// 특정 라벨의 모든 인덱스 캐시 무효화
    pub fn invalidate_label(&self, label: &str) {
        // 라벨과 관련된 모든 exists 캐시 무효화는 복잡하므로
        // all_indexes 캐시만 무효화하고, 개별 조회 시 재검증
        self.all_indexes_l1.clear();
        if let Some(ref l2) = self.all_indexes_l2
            && let Err(e) = l2.clear()
        {
            tracing::warn!("IndexCache all_indexes L2 clear error: {}", e);
        }

        // exists_l1에서 해당 라벨 관련 항목 삭제
        // moka는 조건부 삭제를 직접 지원하지 않으므로 전체 무효화
        // (실제로는 라벨별 세분화된 무효화가 어려움)
        self.exists_l1.clear();
        if let Some(ref l2) = self.exists_l2 {
            let prefix = zeta4j_codec::serialize(&(label.to_string(),)).unwrap_or_default();
            if let Err(e) = l2.invalidate_by_prefix(&prefix) {
                tracing::warn!("IndexCache exists L2 invalidate_by_prefix error: {}", e);
            }
        }

        if self.config.stats_enabled {
            self.stats.record_invalidation();
        }
    }

    /// 특정 인덱스 캐시 무효화
    pub fn invalidate(&self, label: &str, prop: &str) {
        let key = (label.to_string(), prop.to_string());

        self.exists_l1.invalidate(&key);
        if let Some(ref l2) = self.exists_l2
            && let Err(e) = l2.invalidate(&key)
        {
            tracing::warn!("IndexCache exists L2 invalidate error: {}", e);
        }

        // all_indexes 캐시도 무효화 (목록이 변경되었으므로)
        self.all_indexes_l1.clear();
        if let Some(ref l2) = self.all_indexes_l2
            && let Err(e) = l2.clear()
        {
            tracing::warn!("IndexCache all_indexes L2 clear error: {}", e);
        }

        if self.config.stats_enabled {
            self.stats.record_invalidation();
        }
    }

    /// 모든 캐시 무효화
    pub fn clear(&self) {
        self.exists_l1.clear();
        self.all_indexes_l1.clear();

        if let Some(ref l2) = self.exists_l2
            && let Err(e) = l2.clear()
        {
            tracing::warn!("IndexCache exists L2 clear error: {}", e);
        }
        if let Some(ref l2) = self.all_indexes_l2
            && let Err(e) = l2.clear()
        {
            tracing::warn!("IndexCache all_indexes L2 clear error: {}", e);
        }
    }

    /// 캐시 통계 반환
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
}
