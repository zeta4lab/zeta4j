// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 2-tier 캐시 구현
//!
//! L1 (moka 인메모리) + L2 (redb 디스크) 계층 구조

use crate::config::CacheConfig;
use crate::error::Result;
use crate::l1_moka::L1Cache;
use crate::l2_redb::L2Cache;
use crate::stats::CacheStats;
use serde::{Serialize, de::DeserializeOwned};
use std::hash::Hash;
use std::sync::Arc;

/// 2-tier 캐시
///
/// L1 (moka): 고속 인메모리 LRU 캐시
/// L2 (redb): 영구 디스크 캐시 (서버 재시작 후에도 유지)
pub struct TieredCache<K, V>
where
    K: Hash + Eq + Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
    V: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    l1: L1Cache<K, V>,
    l2: Option<L2Cache<K, V>>,
    config: CacheConfig,
    stats: Arc<CacheStats>,
}

impl<K, V> TieredCache<K, V>
where
    K: Hash + Eq + Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
    V: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    /// 새 TieredCache 생성
    ///
    /// # Arguments
    /// * `config` - 캐시 설정
    /// * `db` - 공유 redb 데이터베이스
    /// * `tree_name` - redb 테이블 이름
    pub fn new(
        config: CacheConfig,
        db: Option<Arc<redb::Database>>,
        tree_name: &'static str,
    ) -> Result<Self> {
        let l1 = L1Cache::new(config.l1_max_capacity, config.l1_ttl);

        let l2 = if config.l2_enabled {
            if let Some(db) = db {
                Some(L2Cache::with_db(db, tree_name)?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            l1,
            l2,
            config,
            stats: Arc::new(CacheStats::new()),
        })
    }

    /// L1 전용 캐시 생성 (메모리만 사용)
    pub fn l1_only(config: CacheConfig) -> Self {
        let l1 = L1Cache::new(config.l1_max_capacity, config.l1_ttl);

        Self {
            l1,
            l2: None,
            config,
            stats: Arc::new(CacheStats::new()),
        }
    }

    /// 값 조회
    ///
    /// L1 히트 → L2 히트 → None 순으로 검색
    pub fn get(&self, key: &K) -> Option<V> {
        // L1 체크
        if let Some(value) = self.l1.get(key) {
            if self.config.stats_enabled {
                self.stats.record_l1_hit();
            }
            return Some(value);
        }

        // L2 체크
        if let Some(ref l2) = self.l2 {
            match l2.get(key) {
                Ok(Some(value)) => {
                    if self.config.stats_enabled {
                        self.stats.record_l2_hit();
                    }
                    // L1에 승격 (promote)
                    self.l1.insert(key.clone(), value.clone());
                    return Some(value);
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("L2 cache get error: {}", e);
                }
            }
        }

        if self.config.stats_enabled {
            self.stats.record_miss();
        }
        None
    }

    /// 값 조회 또는 생성
    ///
    /// 캐시에 없으면 fetch 함수를 호출하여 값을 생성하고 캐시에 저장
    pub fn get_or_insert<F>(&self, key: K, fetch: F) -> V
    where
        F: FnOnce() -> V,
    {
        // 먼저 조회 시도
        if let Some(value) = self.get(&key) {
            return value;
        }

        // 캐시 미스: fetch 실행 및 저장
        let value = fetch();
        self.insert(key, value.clone());
        value
    }

    /// 값 삽입 (L1 + L2)
    pub fn insert(&self, key: K, value: V) {
        // L1 삽입
        self.l1.insert(key.clone(), value.clone());

        // L2 삽입
        if let Some(ref l2) = self.l2
            && let Err(e) = l2.insert(&key, &value)
        {
            tracing::warn!("L2 cache insert error: {}", e);
        }

        if self.config.stats_enabled {
            self.stats.record_insert();
        }
    }

    /// 값 무효화 (L1 + L2)
    pub fn invalidate(&self, key: &K) {
        // L1 무효화
        self.l1.invalidate(key);

        // L2 무효화
        if let Some(ref l2) = self.l2
            && let Err(e) = l2.invalidate(key)
        {
            tracing::warn!("L2 cache invalidate error: {}", e);
        }

        if self.config.stats_enabled {
            self.stats.record_invalidation();
        }
    }

    /// 모든 값 무효화
    pub fn clear(&self) {
        self.l1.clear();

        if let Some(ref l2) = self.l2
            && let Err(e) = l2.clear()
        {
            tracing::warn!("L2 cache clear error: {}", e);
        }
    }

    /// 캐시 통계 반환
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// L1 캐시 항목 수
    pub fn l1_len(&self) -> usize {
        self.l1.len()
    }

    /// L2 캐시 항목 수
    pub fn l2_len(&self) -> Result<Option<usize>> {
        self.l2.as_ref().map(L2Cache::len).transpose()
    }
}
