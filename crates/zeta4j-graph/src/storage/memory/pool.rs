//! 메모리 풀 관리 시스템 (설계 문서 준수)

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::limiter::{MemoryCategory, MemoryLimiter, MemoryReservation};

/// 메모리 풀 통계
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    pub total_allocations: usize,
    pub total_deallocations: usize,
    pub current_pool_size: usize,
    pub peak_pool_size: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub memory_saved: usize,
}

/// 풀링된 객체의 생명주기 관리
pub trait Poolable: Send + Sync {
    /// 객체를 풀에서 가져올 때 호출 (재초기화)
    fn reset(&mut self);

    /// 객체를 풀로 반환할 때 호출 (정리)
    fn prepare_for_pool(&mut self);

    /// 객체의 메모리 크기 반환 (통계용)
    fn memory_size(&self) -> usize;
}

/// 풀 가능한 객체를 위한 래퍼
pub struct PoolGuard<T: Poolable + 'static> {
    object: Option<T>,
    pool: Arc<MemoryPool<T>>,
}

impl<T: Poolable + 'static> PoolGuard<T> {
    pub fn new(object: T, pool: Arc<MemoryPool<T>>) -> Self {
        Self {
            object: Some(object),
            pool,
        }
    }
}

impl<T: Poolable + 'static> std::ops::Deref for PoolGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let Some(object) = self.object.as_ref() else {
            std::process::abort();
        };
        object
    }
}

impl<T: Poolable + 'static> std::ops::DerefMut for PoolGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let Some(object) = self.object.as_mut() else {
            std::process::abort();
        };
        object
    }
}

impl<T: Poolable + 'static> Drop for PoolGuard<T> {
    fn drop(&mut self) {
        if let Some(mut object) = self.object.take() {
            object.prepare_for_pool();
            self.pool.return_object(object);
        }
    }
}

/// 고성능 메모리 풀 (설계 문서 아키텍처)
pub struct MemoryPool<T: Poolable + 'static> {
    /// 사용 가능한 객체들
    pool: Mutex<Vec<PooledEntry<T>>>,

    /// 객체 생성 팩토리
    factory: Box<dyn Fn() -> T + Send + Sync>,

    /// 최대 풀 크기
    max_size: usize,

    /// 사용 통계 (획득 횟수)
    stats: AtomicUsize,

    /// 현재 풀 크기 (atomic for stats)
    current_size: AtomicUsize,

    /// 상세 통계 정보
    detailed_stats: Mutex<PoolStats>,

    /// 마지막 정리 시점
    last_cleanup: Mutex<Instant>,

    /// 정리 간격
    cleanup_interval: Duration,

    /// Optional database ledger. Disabled pools have no reservation overhead.
    memory_limiter: Option<Arc<MemoryLimiter>>,
}

struct PooledEntry<T> {
    object: T,
    reservation: Option<MemoryReservation>,
}

impl<T: Poolable + 'static> MemoryPool<T> {
    /// 새 메모리 풀 생성
    pub fn new<F>(factory: F, max_size: usize) -> Arc<Self>
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        Self::new_with_limiter(factory, max_size, None)
    }

    pub fn new_accounted<F>(
        factory: F,
        max_size: usize,
        memory_limiter: Arc<MemoryLimiter>,
    ) -> Arc<Self>
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        Self::new_with_limiter(factory, max_size, Some(memory_limiter))
    }

    fn new_with_limiter<F>(
        factory: F,
        max_size: usize,
        memory_limiter: Option<Arc<MemoryLimiter>>,
    ) -> Arc<Self>
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        Arc::new(Self {
            pool: Mutex::new(Vec::with_capacity(max_size)),
            factory: Box::new(factory),
            max_size,
            stats: AtomicUsize::new(0),
            current_size: AtomicUsize::new(0),
            detailed_stats: Mutex::new(PoolStats::default()),
            last_cleanup: Mutex::new(Instant::now()),
            cleanup_interval: Duration::from_secs(60), // 1분마다 정리
            memory_limiter,
        })
    }

    /// 기본 설정으로 메모리 풀 생성
    pub fn with_default<F>(factory: F) -> Arc<Self>
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        Self::new(factory, 1000) // 기본 최대 크기: 1000개
    }

    /// 객체 가져오기 - O(1) 성능
    pub fn acquire(self: &Arc<Self>) -> PoolGuard<T> {
        let object = {
            let mut pool = self.pool.lock();
            match pool.pop() {
                Some(entry) => {
                    let mut object = entry.object;
                    object.reset();
                    self.current_size.fetch_sub(1, Ordering::Relaxed);

                    // 통계 업데이트
                    {
                        let mut stats = self.detailed_stats.lock();
                        stats.cache_hits += 1;
                    }

                    object
                }
                _ => {
                    // 풀이 비어있으면 새로 생성
                    let obj = (self.factory)();

                    // 통계 업데이트
                    {
                        let mut stats = self.detailed_stats.lock();
                        stats.total_allocations += 1;
                        stats.cache_misses += 1;
                    }

                    obj
                }
            }
        };

        // 획득 통계 업데이트
        self.stats.fetch_add(1, Ordering::Relaxed);

        PoolGuard::new(object, Arc::clone(self))
    }

    /// 객체를 풀로 반환 - O(1) 성능 (내부 사용)
    fn return_object(&self, object: T) {
        let current_pool_size = self.current_size.load(Ordering::Relaxed);
        let memory_size = object.memory_size();
        let reservation = match &self.memory_limiter {
            Some(limiter) => {
                let Ok(reservation) = limiter.try_reserve(MemoryCategory::ObjectPool, memory_size)
                else {
                    return;
                };
                Some(reservation)
            }
            None => None,
        };

        if current_pool_size < self.max_size {
            let mut pool = self.pool.lock();

            // 재확인 (다른 스레드가 추가했을 수 있음)
            if pool.len() < self.max_size {
                pool.push(PooledEntry {
                    object,
                    reservation,
                });
                self.current_size.fetch_add(1, Ordering::Relaxed);

                // 통계 업데이트
                {
                    let mut stats = self.detailed_stats.lock();
                    stats.total_deallocations += 1;
                    stats.current_pool_size = pool.len();
                    stats.peak_pool_size = stats.peak_pool_size.max(pool.len());
                    stats.memory_saved += memory_size;
                }
            }
        }

        // 주기적 정리
        self.try_cleanup();
    }

    /// 풀 크기 조회
    pub fn current_size(&self) -> usize {
        self.current_size.load(Ordering::Relaxed)
    }

    /// 최대 풀 크기
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// 통계 정보 조회
    pub fn stats(&self) -> PoolStats {
        self.detailed_stats.lock().clone()
    }

    /// 풀 정리 - 메모리 최적화
    pub fn cleanup(&self) {
        let mut pool = self.pool.lock();
        let target_size = pool.len() / 2;

        while pool.len() > target_size {
            pool.pop();
            self.current_size.fetch_sub(1, Ordering::Relaxed);
        }

        // 벡터 축소
        pool.shrink_to_fit();

        // 통계 업데이트
        {
            let mut stats = self.detailed_stats.lock();
            stats.current_pool_size = pool.len();
        }

        *self.last_cleanup.lock() = Instant::now();
    }

    /// 조건부 정리 시도
    fn try_cleanup(&self) {
        let last_cleanup = *self.last_cleanup.lock();
        if last_cleanup.elapsed() >= self.cleanup_interval {
            self.cleanup();
        }
    }

    /// 풀 완전 비우기
    pub fn clear(&self) {
        let mut pool = self.pool.lock();
        pool.clear();
        self.current_size.store(0, Ordering::Relaxed);

        // 통계 초기화
        {
            let mut stats = self.detailed_stats.lock();
            stats.current_pool_size = 0;
        }
    }

    /// 풀 사전 할당 - 동적 확장 지원
    pub fn preallocate(&self, count: usize) {
        let target_count = (count).min(self.max_size);

        for _ in 0..target_count {
            if self.current_size() >= self.max_size {
                break;
            }

            let object = (self.factory)();
            let memory_size = object.memory_size();
            let reservation = match self.memory_limiter.as_ref() {
                Some(limiter) => {
                    let Ok(reservation) =
                        limiter.try_reserve(MemoryCategory::ObjectPool, memory_size)
                    else {
                        break;
                    };
                    Some(reservation)
                }
                None => None,
            };
            let mut pool = self.pool.lock();
            if pool.len() >= self.max_size {
                break;
            }
            pool.push(PooledEntry {
                object,
                reservation,
            });
            self.current_size.fetch_add(1, Ordering::Relaxed);
        }

        // 통계 업데이트
        {
            let pool_size = self.pool.lock().len();
            let mut stats = self.detailed_stats.lock();
            stats.current_pool_size = pool_size;
            stats.peak_pool_size = stats.peak_pool_size.max(pool_size);
        }
    }

    /// 메모리 사용량 추정 (바이트)
    pub fn estimated_memory_usage(&self) -> usize {
        let pool_size = self.current_size();
        if pool_size == 0 {
            return 0;
        }

        let pool = self.pool.lock();
        pool.iter()
            .map(|entry| {
                entry
                    .reservation
                    .as_ref()
                    .map_or_else(|| entry.object.memory_size(), MemoryReservation::bytes)
            })
            .fold(0usize, usize::saturating_add)
    }

    /// 풀이 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.current_size() == 0
    }

    /// 풀이 가득 찼는지 확인
    pub fn is_full(&self) -> bool {
        self.current_size() >= self.max_size
    }
}

impl<T: Poolable + 'static> Drop for MemoryPool<T> {
    fn drop(&mut self) {
        self.clear();
    }
}

/// 메모리 풀 관리자 (전역 관리)
pub struct MemoryPoolManager {
    pools: Mutex<std::collections::HashMap<&'static str, Box<dyn std::any::Any + Send + Sync>>>,
}

impl MemoryPoolManager {
    pub fn new() -> Self {
        Self {
            pools: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// 풀 등록
    pub fn register_pool<T: Poolable + 'static>(
        &self,
        name: &'static str,
        pool: Arc<MemoryPool<T>>,
    ) {
        let mut pools = self.pools.lock();
        pools.insert(name, Box::new(pool));
    }

    /// 풀 조회
    pub fn get_pool<T: Poolable + 'static>(
        &self,
        name: &'static str,
    ) -> Option<Arc<MemoryPool<T>>> {
        let pools = self.pools.lock();
        pools
            .get(name)?
            .downcast_ref::<Arc<MemoryPool<T>>>()
            .cloned()
    }

    /// 모든 풀 정리
    pub fn cleanup_all(&self) {
        // 타입 지우기로 인해 직접 정리는 복잡함
        // 실제로는 각 풀별로 정리 스레드를 운영하거나
        // 특정 인터페이스를 통해 정리하는 것이 좋음
    }
}

impl Default for MemoryPoolManager {
    fn default() -> Self {
        Self::new()
    }
}

use std::sync::LazyLock;

/// 전역 메모리 풀 관리자
pub static GLOBAL_POOL_MANAGER: LazyLock<MemoryPoolManager> = LazyLock::new(MemoryPoolManager::new);

/// 편의 매크로: 풀 등록
#[macro_export]
macro_rules! register_pool {
    ($name:expr_2021, $pool:expr_2021) => {
        $crate::storage::memory::pool::GLOBAL_POOL_MANAGER.register_pool($name, $pool);
    };
}

/// 편의 매크로: 풀 가져오기
#[macro_export]
macro_rules! get_pool {
    ($name:expr_2021, $type:ty) => {
        $crate::storage::memory::pool::GLOBAL_POOL_MANAGER.get_pool::<$type>($name)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::MemoryLimiterConfig;

    #[derive(Default)]
    struct FixedObject;

    impl Poolable for FixedObject {
        fn reset(&mut self) {}

        fn prepare_for_pool(&mut self) {}

        fn memory_size(&self) -> usize {
            64
        }
    }

    #[test]
    fn accounted_pool_releases_evicted_and_checked_out_objects() {
        let limiter = Arc::new(MemoryLimiter::new(MemoryLimiterConfig {
            max_memory_bytes: 1024,
            auto_cleanup_enabled: false,
            ..MemoryLimiterConfig::default()
        }));
        let pool = MemoryPool::new_accounted(FixedObject::default, 4, Arc::clone(&limiter));
        pool.preallocate(4);
        assert_eq!(pool.estimated_memory_usage(), 256);
        assert_eq!(limiter.current_usage(), 256);

        let checked_out = pool.acquire();
        assert_eq!(pool.estimated_memory_usage(), 192);
        assert_eq!(limiter.current_usage(), 192);
        drop(checked_out);
        assert_eq!(limiter.current_usage(), 256);

        pool.cleanup();
        assert_eq!(pool.estimated_memory_usage(), 128);
        assert_eq!(limiter.current_usage(), 128);
        pool.clear();
        assert_eq!(limiter.current_usage(), 0);
    }

    #[test]
    fn admission_cleanup_reclaims_pool_once_then_retries() -> Result<(), Box<dyn std::error::Error>>
    {
        let limiter = Arc::new(MemoryLimiter::new(MemoryLimiterConfig {
            max_memory_bytes: 300,
            auto_cleanup_enabled: true,
            ..MemoryLimiterConfig::default()
        }));
        let pool = MemoryPool::new_accounted(FixedObject::default, 4, Arc::clone(&limiter));
        pool.preallocate(4);
        let cleanup_pool = Arc::clone(&pool);
        limiter.set_cleanup_callback(move || cleanup_pool.cleanup());

        let query = limiter.try_reserve(MemoryCategory::Query, 160)?;
        assert_eq!(limiter.current_usage(), 288);
        let stats = limiter.stats();
        assert_eq!(stats.auto_cleanup_count, 1);
        assert_eq!(stats.cleanup_succeeded_count, 1);
        assert_eq!(stats.cleanup_reclaimed_bytes, 128);
        drop(query);
        pool.clear();
        assert_eq!(limiter.current_usage(), 0);
        Ok(())
    }

    #[test]
    fn cleanup_reclaims_small_idle_pool_independent_of_capacity() {
        let limiter = Arc::new(MemoryLimiter::new(MemoryLimiterConfig {
            max_memory_bytes: 1024,
            auto_cleanup_enabled: false,
            ..MemoryLimiterConfig::default()
        }));
        let pool = MemoryPool::new_accounted(FixedObject::default, 1000, Arc::clone(&limiter));
        pool.preallocate(3);
        assert_eq!(pool.current_size(), 3);
        assert_eq!(limiter.current_usage(), 192);

        pool.cleanup();

        assert_eq!(pool.current_size(), 1);
        assert_eq!(limiter.current_usage(), 64);
    }
}
