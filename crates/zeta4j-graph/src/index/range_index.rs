//! 범위 검색을 위한 B+Tree 기반 인덱스
//!
//! 정수 및 실수 값에 대한 효율적인 범위 검색을 지원합니다.
//! 시간 복잡도: O(log n + k) (k = 결과 수)

use std::collections::{BTreeMap, HashSet};
use std::hash::Hash;
use std::ops::Bound;

use ordered_float::OrderedFloat;
use parking_lot::RwLock;

/// B+Tree 기반 범위 검색 인덱스
///
/// 기존 Vec 기반 RangeIndex보다 효율적인 O(log n + k) 범위 검색 지원
#[derive(Debug)]
pub struct BTreeRangeIndex<V>
where
    V: Clone + Eq + Hash,
{
    /// 정수 값 인덱스: i64 -> value set
    integer_index: RwLock<BTreeMap<i64, HashSet<V>>>,

    /// 실수 값 인덱스: OrderedFloat<f64> -> value set
    float_index: RwLock<BTreeMap<OrderedFloat<f64>, HashSet<V>>>,
}

impl<V> Default for BTreeRangeIndex<V>
where
    V: Clone + Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<V> BTreeRangeIndex<V>
where
    V: Clone + Eq + Hash,
{
    /// 새로운 범위 인덱스 생성
    pub fn new() -> Self {
        Self {
            integer_index: RwLock::new(BTreeMap::new()),
            float_index: RwLock::new(BTreeMap::new()),
        }
    }

    // ========== 정수 인덱스 ==========

    /// 정수 값 삽입
    pub fn insert_integer(&self, key: i64, value: V) {
        let mut index = self.integer_index.write();
        index.entry(key).or_default().insert(value);
    }

    /// 정수 값 삭제
    pub fn remove_integer(&self, key: i64, value: &V) -> bool {
        let mut index = self.integer_index.write();
        if let Some(values) = index.get_mut(&key) {
            let removed = values.remove(value);
            if values.is_empty() {
                index.remove(&key);
            }
            removed
        } else {
            false
        }
    }

    /// 정수 범위 검색 (inclusive)
    /// 시간 복잡도: O(log n + k)
    pub fn range_integer(&self, min: i64, max: i64) -> Vec<V> {
        let index = self.integer_index.read();
        let mut results = Vec::new();

        for (_, values) in index.range(min..=max) {
            results.extend(values.iter().cloned());
        }

        results
    }

    /// 정수 범위 검색 (경계 지정)
    pub fn range_integer_bounds(&self, min: Bound<i64>, max: Bound<i64>) -> Vec<V> {
        let index = self.integer_index.read();
        let mut results = Vec::new();

        for (_, values) in index.range((min, max)) {
            results.extend(values.iter().cloned());
        }

        results
    }

    /// 정수 값보다 큰 모든 값 검색
    pub fn greater_than_integer(&self, key: i64, inclusive: bool) -> Vec<V> {
        let index = self.integer_index.read();
        let mut results = Vec::new();

        let range = if inclusive {
            index.range(key..)
        } else {
            index.range((Bound::Excluded(key), Bound::Unbounded))
        };

        for (_, values) in range {
            results.extend(values.iter().cloned());
        }

        results
    }

    /// 정수 값보다 작은 모든 값 검색
    pub fn less_than_integer(&self, key: i64, inclusive: bool) -> Vec<V> {
        let index = self.integer_index.read();
        let mut results = Vec::new();

        let range = if inclusive {
            index.range(..=key)
        } else {
            index.range(..key)
        };

        for (_, values) in range {
            results.extend(values.iter().cloned());
        }

        results
    }

    /// 정수 인덱스 엔트리 수
    pub fn integer_count(&self) -> usize {
        self.integer_index.read().len()
    }

    // ========== 실수 인덱스 ==========

    /// 실수 값 삽입
    pub fn insert_float(&self, key: f64, value: V) {
        let mut index = self.float_index.write();
        index.entry(OrderedFloat(key)).or_default().insert(value);
    }

    /// 실수 값 삭제
    pub fn remove_float(&self, key: f64, value: &V) -> bool {
        let mut index = self.float_index.write();
        let ordered_key = OrderedFloat(key);
        if let Some(values) = index.get_mut(&ordered_key) {
            let removed = values.remove(value);
            if values.is_empty() {
                index.remove(&ordered_key);
            }
            removed
        } else {
            false
        }
    }

    /// 실수 범위 검색 (inclusive)
    /// 시간 복잡도: O(log n + k)
    pub fn range_float(&self, min: f64, max: f64) -> Vec<V> {
        let index = self.float_index.read();
        let mut results = Vec::new();

        let min_ord = OrderedFloat(min);
        let max_ord = OrderedFloat(max);

        for (_, values) in index.range(min_ord..=max_ord) {
            results.extend(values.iter().cloned());
        }

        results
    }

    /// 실수 범위 검색 (경계 지정)
    pub fn range_float_bounds(&self, min: Bound<f64>, max: Bound<f64>) -> Vec<V> {
        let index = self.float_index.read();
        let mut results = Vec::new();

        let min_bound = match min {
            Bound::Included(v) => Bound::Included(OrderedFloat(v)),
            Bound::Excluded(v) => Bound::Excluded(OrderedFloat(v)),
            Bound::Unbounded => Bound::Unbounded,
        };
        let max_bound = match max {
            Bound::Included(v) => Bound::Included(OrderedFloat(v)),
            Bound::Excluded(v) => Bound::Excluded(OrderedFloat(v)),
            Bound::Unbounded => Bound::Unbounded,
        };

        for (_, values) in index.range((min_bound, max_bound)) {
            results.extend(values.iter().cloned());
        }

        results
    }

    /// 실수 값보다 큰 모든 값 검색
    pub fn greater_than_float(&self, key: f64, inclusive: bool) -> Vec<V> {
        let index = self.float_index.read();
        let mut results = Vec::new();
        let ordered_key = OrderedFloat(key);

        let range = if inclusive {
            index.range(ordered_key..)
        } else {
            index.range((Bound::Excluded(ordered_key), Bound::Unbounded))
        };

        for (_, values) in range {
            results.extend(values.iter().cloned());
        }

        results
    }

    /// 실수 값보다 작은 모든 값 검색
    pub fn less_than_float(&self, key: f64, inclusive: bool) -> Vec<V> {
        let index = self.float_index.read();
        let mut results = Vec::new();
        let ordered_key = OrderedFloat(key);

        let range = if inclusive {
            index.range(..=ordered_key)
        } else {
            index.range(..ordered_key)
        };

        for (_, values) in range {
            results.extend(values.iter().cloned());
        }

        results
    }

    /// 실수 인덱스 엔트리 수
    pub fn float_count(&self) -> usize {
        self.float_index.read().len()
    }

    // ========== 공통 ==========

    /// 모든 인덱스 초기화
    pub fn clear(&self) {
        self.integer_index.write().clear();
        self.float_index.write().clear();
    }

    /// 인덱스가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.integer_index.read().is_empty() && self.float_index.read().is_empty()
    }

    /// 총 엔트리 수
    pub fn len(&self) -> usize {
        self.integer_count() + self.float_count()
    }
}

use crate::core::Value;
use crate::index::HashIndex;

/// 범위 검색 fallback 헬퍼 (정수용)
///
/// BTreeRangeIndex가 없을 때 HashIndex를 스캔하여 정수 범위 검색 수행
/// 시간 복잡도: O(n)
pub fn fallback_integer_range_search<V: Clone + Eq + std::hash::Hash + Ord>(
    index: &HashIndex<Value, V>,
    min: i64,
    max: i64,
) -> Vec<V> {
    let mut results = Vec::new();
    for key in index.keys() {
        if let Some(int_val) = key.as_integer()
            && int_val >= min
            && int_val <= max
        {
            results.extend(index.get(&key));
        }
    }
    results
}

/// 범위 검색 fallback 헬퍼 (실수용)
///
/// BTreeRangeIndex가 없을 때 HashIndex를 스캔하여 실수 범위 검색 수행
/// 시간 복잡도: O(n)
pub fn fallback_float_range_search<V: Clone + Eq + std::hash::Hash + Ord>(
    index: &HashIndex<Value, V>,
    min: f64,
    max: f64,
) -> Vec<V> {
    let mut results = Vec::new();
    for key in index.keys() {
        if let Some(float_val) = key.as_float()
            && float_val >= min
            && float_val <= max
        {
            results.extend(index.get(&key));
        }
    }
    results
}
