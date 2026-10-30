//! 스키마 인덱스 힌트
//!
//! 스키마에 인덱스 생성 힌트를 정의합니다.
//!
//! # v1.59.0
//!
//! - IndexType: 인덱스 타입 (Hash, Range, Fulltext, Spatial, Composite)
//! - IndexHint: 인덱스 생성 힌트
//! - 스키마 기반 인덱스 자동 생성 지원
//!
//! # 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::schema::{NodeLabelSchema, IndexHint, IndexType, PropertyDefinition, PropertyType};
//!
//! // 인덱스 힌트가 포함된 스키마
//! let schema = NodeLabelSchema::new("Person")
//!     .with_property(PropertyDefinition::new("name", PropertyType::String))
//!     .with_property(PropertyDefinition::new("email", PropertyType::String))
//!     .with_property(PropertyDefinition::new("age", PropertyType::Integer))
//!     .with_property(PropertyDefinition::new("bio", PropertyType::String))
//!     .with_property(PropertyDefinition::new("location", PropertyType::Point))
//!     // 단일 프로퍼티 인덱스
//!     .with_index(IndexHint::hash("email").unique())
//!     .with_index(IndexHint::range("age"))
//!     // 전문 검색 인덱스
//!     .with_index(IndexHint::fulltext("bio"))
//!     // 공간 인덱스
//!     .with_index(IndexHint::spatial("location"))
//!     // 복합 인덱스
//!     .with_index(IndexHint::composite(vec!["name", "age"]));
//! ```

use serde::{Deserialize, Serialize};

/// 인덱스 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum IndexType {
    /// 해시 인덱스 - 동등 비교 최적화
    #[default]
    Hash,
    /// 범위 인덱스 (B-Tree) - 범위 쿼리 최적화
    Range,
    /// 전문 검색 인덱스 - 텍스트 검색 최적화
    Fulltext,
    /// 공간 인덱스 (R-Tree) - 공간 쿼리 최적화
    Spatial,
    /// 복합 인덱스 - 여러 프로퍼티 조합 인덱스
    Composite,
}

impl IndexType {
    /// 인덱스 타입 이름
    pub fn name(&self) -> &'static str {
        match self {
            IndexType::Hash => "HASH",
            IndexType::Range => "RANGE",
            IndexType::Fulltext => "FULLTEXT",
            IndexType::Spatial => "SPATIAL",
            IndexType::Composite => "COMPOSITE",
        }
    }

    /// Cypher 스타일 표현
    pub fn to_cypher(&self) -> &'static str {
        match self {
            IndexType::Hash => "LOOKUP",
            IndexType::Range => "RANGE",
            IndexType::Fulltext => "FULLTEXT",
            IndexType::Spatial => "POINT",
            IndexType::Composite => "RANGE",
        }
    }
}

/// 전문 검색 분석기 타입
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum FulltextAnalyzer {
    /// 표준 분석기 (공백/구두점 분리)
    #[default]
    Standard,
    /// 간단한 분석기 (소문자 변환만)
    Simple,
    /// 공백 분석기 (공백 분리만)
    Whitespace,
    /// 언어별 분석기
    Language(String),
    /// 커스텀 분석기
    Custom(String),
}

/// 인덱스 힌트
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexHint {
    /// 인덱스 이름 (선택적, 없으면 자동 생성)
    pub name: Option<String>,
    /// 인덱싱할 프로퍼티 목록
    pub properties: Vec<String>,
    /// 인덱스 타입
    pub index_type: IndexType,
    /// UNIQUE 제약조건 여부
    pub unique: bool,
    /// 전문 검색 분석기 (Fulltext 타입일 때만 사용)
    pub analyzer: Option<FulltextAnalyzer>,
}

impl IndexHint {
    /// 새 인덱스 힌트 생성
    pub fn new(properties: Vec<String>, index_type: IndexType) -> Self {
        Self {
            name: None,
            properties,
            index_type,
            unique: false,
            analyzer: None,
        }
    }

    /// 단일 프로퍼티 해시 인덱스
    pub fn hash(property: &str) -> Self {
        Self::new(vec![property.to_string()], IndexType::Hash)
    }

    /// 단일 프로퍼티 범위 인덱스
    pub fn range(property: &str) -> Self {
        Self::new(vec![property.to_string()], IndexType::Range)
    }

    /// 전문 검색 인덱스
    pub fn fulltext(property: &str) -> Self {
        Self {
            name: None,
            properties: vec![property.to_string()],
            index_type: IndexType::Fulltext,
            unique: false,
            analyzer: Some(FulltextAnalyzer::Standard),
        }
    }

    /// 공간 인덱스
    pub fn spatial(property: &str) -> Self {
        Self::new(vec![property.to_string()], IndexType::Spatial)
    }

    /// 복합 인덱스 (여러 프로퍼티)
    pub fn composite<S: Into<String>>(properties: Vec<S>) -> Self {
        Self::new(
            properties.into_iter().map(|s| s.into()).collect(),
            IndexType::Composite,
        )
    }

    /// 인덱스 이름 설정
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    /// UNIQUE 제약조건 설정
    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }

    /// 전문 검색 분석기 설정
    pub fn with_analyzer(mut self, analyzer: FulltextAnalyzer) -> Self {
        self.analyzer = Some(analyzer);
        self
    }

    /// 인덱스 이름 반환 (없으면 자동 생성)
    pub fn get_name(&self, label: &str) -> String {
        if let Some(name) = &self.name {
            name.clone()
        } else {
            // 자동 생성: idx_{label}_{prop1}_{prop2}...
            let props = self.properties.join("_");
            format!("idx_{}_{}", label.to_lowercase(), props)
        }
    }

    /// 단일 프로퍼티 인덱스인지 확인
    pub fn is_single_property(&self) -> bool {
        self.properties.len() == 1
    }

    /// 복합 인덱스인지 확인
    pub fn is_composite(&self) -> bool {
        self.properties.len() > 1 || self.index_type == IndexType::Composite
    }

    /// Cypher CREATE INDEX 문 생성
    ///
    /// # Arguments
    /// * `label` - 노드 라벨 또는 관계 타입
    /// * `is_node` - 노드 인덱스 여부 (false면 관계 인덱스)
    pub fn to_cypher(&self, label: &str, is_node: bool) -> String {
        let index_name = self.get_name(label);
        let entity = if is_node { "n" } else { "r" };
        let entity_type = "FOR";

        let props: Vec<String> = self
            .properties
            .iter()
            .map(|p| format!("{}.{}", entity, p))
            .collect();
        let props_str = if props.len() == 1 {
            format!("({})", props[0])
        } else {
            format!("({})", props.join(", "))
        };

        let unique_str = if self.unique { " UNIQUE" } else { "" };
        let index_type_str = self.index_type.to_cypher();

        if is_node {
            format!(
                "CREATE{} {} INDEX {} {} ({}:{}) ON {}",
                unique_str, index_type_str, index_name, entity_type, entity, label, props_str
            )
        } else {
            format!(
                "CREATE{} {} INDEX {} {} ()-[{}:{}]-() ON {}",
                unique_str, index_type_str, index_name, entity_type, entity, label, props_str
            )
        }
    }
}

/// 인덱스 힌트 컬렉션
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IndexHints {
    /// 인덱스 힌트 목록
    pub hints: Vec<IndexHint>,
}

impl IndexHints {
    /// 새 컬렉션 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 인덱스 힌트 추가
    pub fn add(&mut self, hint: IndexHint) {
        self.hints.push(hint);
    }

    /// 빌더 패턴: 인덱스 힌트 추가
    pub fn with_hint(mut self, hint: IndexHint) -> Self {
        self.hints.push(hint);
        self
    }

    /// 인덱스 힌트 수
    pub fn len(&self) -> usize {
        self.hints.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.hints.is_empty()
    }

    /// 특정 프로퍼티에 대한 인덱스 힌트 조회
    pub fn get_for_property(&self, property: &str) -> Option<&IndexHint> {
        self.hints
            .iter()
            .find(|h| h.properties.contains(&property.to_string()))
    }

    /// 특정 타입의 인덱스 힌트 목록
    pub fn get_by_type(&self, index_type: IndexType) -> Vec<&IndexHint> {
        self.hints
            .iter()
            .filter(|h| h.index_type == index_type)
            .collect()
    }

    /// UNIQUE 인덱스 힌트 목록
    pub fn get_unique(&self) -> Vec<&IndexHint> {
        self.hints.iter().filter(|h| h.unique).collect()
    }

    /// 모든 인덱스 힌트 반복자
    pub fn iter(&self) -> impl Iterator<Item = &IndexHint> {
        self.hints.iter()
    }
}

impl IntoIterator for IndexHints {
    type Item = IndexHint;
    type IntoIter = std::vec::IntoIter<IndexHint>;

    fn into_iter(self) -> Self::IntoIter {
        self.hints.into_iter()
    }
}

impl<'a> IntoIterator for &'a IndexHints {
    type Item = &'a IndexHint;
    type IntoIter = std::slice::Iter<'a, IndexHint>;

    fn into_iter(self) -> Self::IntoIter {
        self.hints.iter()
    }
}

// ============================================================================
// 단위 테스트
