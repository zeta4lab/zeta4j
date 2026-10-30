//! 인덱스 조건
//!
//! 인덱스 스캔에 사용되는 조건들

use super::super::expr::Expression;

/// 범위 경계
#[derive(Debug, Clone, PartialEq)]
pub struct RangeBound {
    /// 경계값
    pub value: Expression,
    /// 포함 여부 (>= vs >, <= vs <)
    pub inclusive: bool,
}

impl RangeBound {
    /// 새 범위 경계 생성
    pub fn new(value: Expression, inclusive: bool) -> Self {
        Self { value, inclusive }
    }

    /// 포함 경계 (>=, <=)
    pub fn inclusive(value: Expression) -> Self {
        Self::new(value, true)
    }

    /// 제외 경계 (>, <)
    pub fn exclusive(value: Expression) -> Self {
        Self::new(value, false)
    }
}

/// 인덱스 조건
#[derive(Debug, Clone, PartialEq)]
pub enum IndexCondition {
    /// 등호 조건: property = value
    Equality { property: String, value: Expression },
    /// 범위 조건: property > lower AND property < upper
    Range {
        property: String,
        lower: Option<RangeBound>,
        upper: Option<RangeBound>,
    },
}

impl IndexCondition {
    /// 등호 조건 생성
    pub fn equality(property: impl Into<String>, value: Expression) -> Self {
        IndexCondition::Equality {
            property: property.into(),
            value,
        }
    }

    /// 범위 조건 생성 (하한만)
    pub fn greater_than(property: impl Into<String>, value: Expression, inclusive: bool) -> Self {
        IndexCondition::Range {
            property: property.into(),
            lower: Some(RangeBound::new(value, inclusive)),
            upper: None,
        }
    }

    /// 범위 조건 생성 (상한만)
    pub fn less_than(property: impl Into<String>, value: Expression, inclusive: bool) -> Self {
        IndexCondition::Range {
            property: property.into(),
            lower: None,
            upper: Some(RangeBound::new(value, inclusive)),
        }
    }

    /// 범위 조건 생성 (하한+상한)
    pub fn between(property: impl Into<String>, lower: RangeBound, upper: RangeBound) -> Self {
        IndexCondition::Range {
            property: property.into(),
            lower: Some(lower),
            upper: Some(upper),
        }
    }

    /// 프로퍼티 이름 반환
    pub fn property(&self) -> &str {
        match self {
            IndexCondition::Equality { property, .. } => property,
            IndexCondition::Range { property, .. } => property,
        }
    }

    /// 범위 조건인지 확인
    pub fn is_range(&self) -> bool {
        matches!(self, IndexCondition::Range { .. })
    }
}
