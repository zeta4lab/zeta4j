//! 인덱스 트레이트 정의
//!
//! B+ 트리, 해시 인덱스 등 다양한 인덱스 구현체를 위한 공통 인터페이스를 정의합니다.

use crate::error::Result;

/// 키-값 인덱스 트레이트
///
/// 단일 키에 단일 값을 매핑하는 인덱스의 기본 인터페이스입니다.
/// B+ 트리, 해시 인덱스 등이 이 트레이트를 구현합니다.
///
/// # 타입 파라미터
///
/// - `K`: 키 타입 (Ord + Clone 필요)
/// - `V`: 값 타입 (Clone 필요)
///
/// # 예시
///
/// ```ignore
/// use zeta4j_graph::storage::index::Index;
///
/// fn count_entries<I: Index<u64, u64>>(index: &I) -> usize {
///     index.len()
/// }
/// ```
pub trait Index<K, V>: Send + Sync
where
    K: Ord + Clone,
    V: Clone,
{
    /// 키-값 쌍 삽입
    ///
    /// 키가 이미 존재하면 값을 업데이트합니다.
    ///
    /// # Returns
    /// - `Ok(true)`: 새 키 삽입
    /// - `Ok(false)`: 기존 키 업데이트
    fn insert(&mut self, key: K, value: V) -> Result<bool>;

    /// 키로 값 조회
    fn get(&self, key: &K) -> Result<Option<V>>;

    /// 키-값 쌍 삭제
    ///
    /// # Returns
    /// - `Ok(Some(value))`: 삭제된 값
    /// - `Ok(None)`: 키가 존재하지 않음
    fn delete(&mut self, key: &K) -> Result<Option<V>>;

    /// 키 존재 여부 확인
    fn contains(&self, key: &K) -> bool {
        self.get(key).ok().flatten().is_some()
    }

    /// 인덱스 내 엔트리 개수
    fn len(&self) -> usize;

    /// 인덱스가 비어있는지 확인
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 인덱스 초기화 (모든 엔트리 삭제)
    fn clear(&mut self) -> Result<()>;
}

/// 범위 검색 지원 인덱스 트레이트
///
/// B+ 트리처럼 정렬된 키를 지원하는 인덱스에 대해 범위 검색을 제공합니다.
pub trait RangeIndex<K, V>: Index<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    /// 범위 검색 (start_key 이상, end_key 이하)
    ///
    /// # Arguments
    /// - `start_key`: 범위 시작 (포함)
    /// - `end_key`: 범위 끝 (포함)
    ///
    /// # Returns
    /// 정렬된 (키, 값) 쌍의 벡터
    fn range(&self, start_key: &K, end_key: &K) -> Result<Vec<(K, V)>>;

    /// 특정 키 이상의 모든 엔트리 조회
    fn range_from(&self, start_key: &K) -> Result<Vec<(K, V)>>;

    /// 특정 키 이하의 모든 엔트리 조회
    fn range_to(&self, end_key: &K) -> Result<Vec<(K, V)>>;

    /// 최소 키 조회
    fn min_key(&self) -> Result<Option<K>>;

    /// 최대 키 조회
    fn max_key(&self) -> Result<Option<K>>;
}

/// 멀티값 인덱스 트레이트
///
/// 하나의 키에 여러 값을 매핑하는 인덱스입니다.
/// 레이블 인덱스, 역인덱스 등에 사용됩니다.
///
/// # 타입 파라미터
///
/// - `K`: 키 타입
/// - `V`: 값 타입 (단일 값, 하나의 키에 여러 V가 매핑됨)
pub trait MultiValueIndex<K, V>: Send + Sync
where
    K: Clone,
    V: Clone + Eq,
{
    /// 키에 값 추가
    ///
    /// # Returns
    /// - `Ok(true)`: 새 값 추가됨
    /// - `Ok(false)`: 이미 존재하는 값
    fn insert(&mut self, key: K, value: V) -> Result<bool>;

    /// 키의 모든 값 조회
    fn get_all(&self, key: &K) -> Result<Vec<V>>;

    /// 키에서 특정 값 삭제
    ///
    /// # Returns
    /// - `Ok(true)`: 값이 삭제됨
    /// - `Ok(false)`: 해당 값이 존재하지 않음
    fn delete(&mut self, key: &K, value: &V) -> Result<bool>;

    /// 키의 모든 값 삭제
    ///
    /// # Returns
    /// 삭제된 값의 개수
    fn delete_all(&mut self, key: &K) -> Result<usize>;

    /// 키에 특정 값이 존재하는지 확인
    fn contains(&self, key: &K, value: &V) -> Result<bool>;

    /// 키가 존재하는지 확인 (값이 하나라도 있으면 true)
    fn contains_key(&self, key: &K) -> Result<bool>;

    /// 키의 값 개수
    fn count(&self, key: &K) -> Result<usize>;

    /// 전체 엔트리 개수 (모든 키-값 쌍)
    fn total_entries(&self) -> usize;

    /// 고유 키 개수
    fn unique_keys(&self) -> usize;
}

/// 동시성 지원 인덱스 트레이트
///
/// 여러 스레드에서 안전하게 접근할 수 있는 인덱스입니다.
/// Crabbing Protocol, Lock-free 알고리즘 등을 사용합니다.
pub trait ConcurrentIndex<K, V>: Send + Sync
where
    K: Ord + Clone + Send,
    V: Clone + Send,
{
    /// 동시성 안전한 삽입
    fn concurrent_insert(&self, key: K, value: V) -> Result<bool>;

    /// 동시성 안전한 조회
    fn concurrent_get(&self, key: &K) -> Result<Option<V>>;

    /// 동시성 안전한 삭제
    fn concurrent_delete(&self, key: &K) -> Result<Option<V>>;

    /// 동시성 안전한 범위 검색
    fn concurrent_range(&self, start_key: &K, end_key: &K) -> Result<Vec<(K, V)>>;
}

/// 영속성 지원 인덱스 트레이트
///
/// 디스크에 저장되고 복구할 수 있는 인덱스입니다.
pub trait PersistentIndex: Send + Sync {
    /// 인덱스를 디스크에 플러시
    fn flush(&self) -> Result<()>;

    /// 디스크에서 인덱스 복구
    fn recover(&mut self) -> Result<()>;

    /// 인덱스 무결성 검증
    fn verify(&self) -> Result<VerifyResult>;

    /// 인덱스 온라인 재구축
    fn rebuild(&mut self) -> Result<RebuildResult>;
}

/// 인덱스 검증 결과
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// 검증 성공 여부
    pub success: bool,
    /// 총 엔트리 수
    pub total_entries: usize,
    /// 발견된 오류 수
    pub error_count: usize,
    /// 오류 메시지 목록
    pub errors: Vec<String>,
}

impl Default for VerifyResult {
    fn default() -> Self {
        Self {
            success: true,
            total_entries: 0,
            error_count: 0,
            errors: Vec::new(),
        }
    }
}

/// 인덱스 재구축 결과
#[derive(Debug, Clone)]
pub struct RebuildResult {
    /// 재구축 성공 여부
    pub success: bool,
    /// 재구축된 엔트리 수
    pub entries_rebuilt: usize,
    /// 재구축 소요 시간 (밀리초)
    pub duration_ms: u64,
}

impl Default for RebuildResult {
    fn default() -> Self {
        Self {
            success: true,
            entries_rebuilt: 0,
            duration_ms: 0,
        }
    }
}
