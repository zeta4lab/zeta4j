//! 복합 인덱스 키 모듈

use crate::core::Value;
use std::cmp::Ordering;

/// 복합 인덱스 키
///
/// 여러 속성 값을 조합하여 하나의 키로 사용합니다.
/// B-Tree 인덱스에서 정렬 및 범위 검색을 지원합니다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompositeKey {
    /// 키 값들 (속성 순서대로)
    values: Vec<Value>,
}

impl CompositeKey {
    /// 새 복합 키 생성
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    /// 빈 키 생성
    pub fn empty() -> Self {
        Self { values: Vec::new() }
    }

    /// 단일 값으로 키 생성
    pub fn single(value: Value) -> Self {
        Self {
            values: vec![value],
        }
    }

    /// 값 추가
    pub fn push(&mut self, value: Value) {
        self.values.push(value);
    }

    /// 값 개수
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// 값 참조
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// 특정 인덱스의 값
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    /// 프리픽스 매칭 (앞부분 일치)
    ///
    /// prefix의 모든 값이 self의 앞부분과 일치하면 true
    pub fn matches_prefix(&self, prefix: &[Value]) -> bool {
        if prefix.len() > self.values.len() {
            return false;
        }

        for (i, p) in prefix.iter().enumerate() {
            if &self.values[i] != p {
                return false;
            }
        }

        true
    }

    /// 범위 내에 있는지 확인
    ///
    /// start <= self <= end 인지 확인
    pub fn in_range(&self, start: &CompositeKey, end: &CompositeKey) -> bool {
        self >= start && self <= end
    }

    /// 프리픽스 추출
    pub fn prefix(&self, count: usize) -> CompositeKey {
        let count = count.min(self.values.len());
        CompositeKey::new(self.values[..count].to_vec())
    }
}

/// Value 간 비교 헬퍼
fn compare_values(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        // Null은 항상 가장 작음
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,

        // 같은 타입끼리 비교
        (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),

        // 정수와 실수 간 비교
        (Value::Integer(a), Value::Float(b)) => {
            (*a as f64).partial_cmp(b).unwrap_or(Ordering::Equal)
        }
        (Value::Float(a), Value::Integer(b)) => {
            a.partial_cmp(&(*b as f64)).unwrap_or(Ordering::Equal)
        }

        // DateTime 비교
        (Value::DateTime(a), Value::DateTime(b)) => a.cmp(b),
        (Value::Date(a), Value::Date(b)) => a.cmp(b),
        (Value::Time(a), Value::Time(b)) => a.cmp(b),
        (Value::LocalDateTime(a), Value::LocalDateTime(b)) => a.cmp(b),

        // 다른 타입들은 타입 이름으로 정렬
        (a, b) => a.type_name().cmp(b.type_name()),
    }
}

impl PartialOrd for CompositeKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CompositeKey {
    fn cmp(&self, other: &Self) -> Ordering {
        // 사전순 비교 (첫 번째 속성부터)
        let min_len = self.values.len().min(other.values.len());

        for i in 0..min_len {
            let cmp = compare_values(&self.values[i], &other.values[i]);
            if cmp != Ordering::Equal {
                return cmp;
            }
        }

        // 앞부분이 같으면 길이로 비교
        self.values.len().cmp(&other.values.len())
    }
}

impl From<Vec<Value>> for CompositeKey {
    fn from(values: Vec<Value>) -> Self {
        Self::new(values)
    }
}

impl From<Value> for CompositeKey {
    fn from(value: Value) -> Self {
        Self::single(value)
    }
}
