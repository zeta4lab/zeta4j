//! 스키마 정의 시스템
//!
//! 그래프 데이터의 구조를 정의하고 검증하는 모듈입니다.
//!
//! # 주요 기능
//!
//! - 노드 라벨 스키마 정의
//! - 관계 타입 스키마 정의
//! - 프로퍼티 타입 검증
//! - 제약조건 (UNIQUE, NOT NULL, NODE KEY)
//! - 추론 엔진 (라벨/관계 계층, 특성 추론)
//! - 스키마 진화 관리 (버전 관리, 마이그레이션)
//! - 스키마 직렬화/역직렬화
//! - 인덱스 힌트 정의
//!
//! # v1.58.0 업데이트
//!
//! - 스키마 버전 관리: NodeLabelSchema, RelationshipTypeSchema에 version 필드 추가
//! - 호환성 체크 API: check_compatibility(), is_compatible_with(), diff() 메서드
//!
//! # v1.59.0 업데이트
//!
//! - 인덱스 힌트 통합: IndexHint, IndexHints, IndexType 추가
//! - 스키마에 인덱스 정의 포함: with_index() 메서드
//!
//! # 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::schema::{NodeLabelSchema, PropertyDefinition, PropertyType, IndexHint};
//!
//! // 인덱스 힌트가 포함된 스키마
//! let schema = NodeLabelSchema::new("Person")
//!     .with_property(PropertyDefinition::new("name", PropertyType::String))
//!     .with_property(PropertyDefinition::new("email", PropertyType::String))
//!     .with_property(PropertyDefinition::new("age", PropertyType::Integer))
//!     .with_index(IndexHint::hash("email").unique())
//!     .with_index(IndexHint::range("age"));
//!
//! assert!(schema.has_index_hints());
//! assert!(schema.get_index_for_property("email").is_some());
//! ```

pub mod common;
pub mod constraint;
pub mod evolution;
pub mod index_hint;
pub mod inference;
pub mod manager;
pub mod node_schema;
pub mod rel_schema;
pub mod types;

pub use common::ElementSchema;
pub use constraint::{ConstraintDefinition, ConstraintManager, ConstraintTarget, ConstraintType};
pub use evolution::{
    AffectedItems, CompatibilityLevel, DryRunResult, MigrationOp, MigrationResult, MigrationTarget,
    SchemaEvolutionManager, SchemaMigration, SchemaVersion, TypeConverter,
};
pub use index_hint::{FulltextAnalyzer, IndexHint, IndexHints, IndexType};
pub use inference::{
    InferenceEngine, LabelHierarchy, RelationshipCharacteristics, RelationshipHierarchy,
};
pub use manager::{
    AlterSchemaBuilder, AlterSchemaOp, MigrationBuilder, SchemaManager, SchemaMode, SchemaStats,
};
pub use node_schema::{NodeLabelSchema, PropertyChange, PropertyChangeType, SchemaDiff};
pub use rel_schema::RelationshipTypeSchema;
pub use types::{PropertyDefinition, PropertyType};
