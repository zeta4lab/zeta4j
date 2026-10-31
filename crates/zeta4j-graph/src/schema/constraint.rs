// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 제약조건 엔진
//!
//! 그래프 데이터의 무결성을 보장하는 제약조건을 정의하고 검증합니다.
//!
//! # 지원 제약조건
//!
//! ## 기본 제약
//! - UNIQUE: 프로퍼티 값 고유성 보장
//! - NOT NULL: NULL 값 금지
//! - NODE KEY: 복합 프로퍼티 고유성 + NOT NULL
//!
//! ## 프로퍼티 레벨 제약
//! - RANGE: 숫자 값 범위 제한
//! - LENGTH: 문자열 길이 제한
//! - PATTERN: 정규식 패턴 매칭
//! - ENUM: 허용 값 목록 제한
//! - IMMUTABLE: 값 변경 금지
//! - DEFAULT: 기본값 설정
//!
//! ## 클래스 레벨 제약
//! - CHECK: 조건 표현식 검사
//! - REQUIRE_AT_LEAST_ONE: 최소 하나 필수
//! - MUTUALLY_EXCLUSIVE: 상호 배타적
//!
//! ## 관계 레벨 제약
//! - CARDINALITY: 관계 개수 제한
//! - CASCADE: 연쇄 동작
//! - REFERENTIAL_INTEGRITY: 참조 무결성

use indexmap::IndexMap;

use crate::core::{NodeId, Value};
use crate::error::{GraphError, Result};
use crate::storage::util::lock_helpers::{read_lock, write_lock};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use zeta4j_common::{CascadeAction, ReferentialAction};

/// 제약조건 종류
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConstraintType {
    // ============================================================
    // 기본 제약
    // ============================================================
    /// 프로퍼티 값 고유성 (단일 또는 복합)
    Unique,
    /// NULL 값 금지
    NotNull,
    /// 노드 키 (복합 프로퍼티 고유성 + NOT NULL)
    NodeKey,
    /// 관계 키 (향후 확장)
    RelationshipKey,

    // ============================================================
    // 프로퍼티 레벨 제약
    // ============================================================
    /// 숫자 값 범위 제한
    Range {
        /// 최소값 (None = 무제한)
        min: Option<f64>,
        /// 최대값 (None = 무제한)
        max: Option<f64>,
        /// 경계 포함 여부
        inclusive: bool,
    },
    /// 문자열 길이 제한
    Length {
        /// 최소 길이 (None = 무제한)
        min: Option<usize>,
        /// 최대 길이 (None = 무제한)
        max: Option<usize>,
    },
    /// 정규식 패턴 매칭
    Pattern {
        /// 정규식 패턴
        regex: String,
    },
    /// 허용 값 목록 제한
    Enum {
        /// 허용 값 목록
        values: Vec<String>,
    },
    /// 값 변경 금지
    Immutable,
    /// 기본값 설정
    Default {
        /// 기본값
        value: Value,
    },

    // ============================================================
    // 클래스 레벨 제약
    // ============================================================
    /// 조건 표현식 검사
    Check {
        /// 검사 표현식 (Cypher 표현식 문자열)
        expression: String,
    },
    /// 최소 하나 필수
    RequireAtLeastOne,
    /// 상호 배타적
    MutuallyExclusive,

    // ============================================================
    // 관계 레벨 제약
    // ============================================================
    /// 관계 개수 제한
    Cardinality {
        /// 최소 개수
        min: u32,
        /// 최대 개수 (None = 무제한)
        max: Option<u32>,
    },
    /// 연쇄 동작
    Cascade {
        /// 삭제 시 동작
        action: CascadeAction,
    },
    /// 참조 무결성
    ReferentialIntegrity {
        /// 삭제 시 동작
        on_delete: ReferentialAction,
        /// 수정 시 동작
        on_update: ReferentialAction,
    },
}

impl std::fmt::Display for ConstraintType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // 기본 제약
            ConstraintType::Unique => write!(f, "UNIQUE"),
            ConstraintType::NotNull => write!(f, "NOT NULL"),
            ConstraintType::NodeKey => write!(f, "NODE KEY"),
            ConstraintType::RelationshipKey => write!(f, "RELATIONSHIP KEY"),
            // 프로퍼티 레벨 제약
            ConstraintType::Range {
                min,
                max,
                inclusive,
            } => {
                let inc = if *inclusive { "inclusive" } else { "exclusive" };
                write!(f, "RANGE({:?}, {:?}, {})", min, max, inc)
            }
            ConstraintType::Length { min, max } => {
                write!(f, "LENGTH({:?}, {:?})", min, max)
            }
            ConstraintType::Pattern { regex } => write!(f, "PATTERN('{}')", regex),
            ConstraintType::Enum { values } => write!(f, "ENUM({:?})", values),
            ConstraintType::Immutable => write!(f, "IMMUTABLE"),
            ConstraintType::Default { value } => write!(f, "DEFAULT({:?})", value),
            // 클래스 레벨 제약
            ConstraintType::Check { expression } => write!(f, "CHECK({})", expression),
            ConstraintType::RequireAtLeastOne => write!(f, "REQUIRE AT LEAST ONE"),
            ConstraintType::MutuallyExclusive => write!(f, "MUTUALLY EXCLUSIVE"),
            // 관계 레벨 제약
            ConstraintType::Cardinality { min, max } => {
                write!(f, "CARDINALITY({}, {:?})", min, max)
            }
            ConstraintType::Cascade { action } => write!(f, "ON DELETE {:?}", action),
            ConstraintType::ReferentialIntegrity {
                on_delete,
                on_update,
            } => {
                write!(f, "ON DELETE {:?} ON UPDATE {:?}", on_delete, on_update)
            }
        }
    }
}

/// 제약조건 대상
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConstraintTarget {
    /// 노드 라벨에 대한 제약조건
    NodeLabel(String),
    /// 관계 타입에 대한 제약조건
    RelationshipType(String),
}

impl std::fmt::Display for ConstraintTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstraintTarget::NodeLabel(label) => write!(f, "(:{label})"),
            ConstraintTarget::RelationshipType(rel_type) => write!(f, "-[:{rel_type}]-"),
        }
    }
}

/// 제약조건 정의
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintDefinition {
    /// 제약조건 이름 (고유 식별자)
    pub name: String,
    /// 제약조건 종류
    pub constraint_type: ConstraintType,
    /// 적용 대상 (노드 라벨 또는 관계 타입)
    pub target: ConstraintTarget,
    /// 대상 프로퍼티들 (단일 또는 복합)
    pub properties: Vec<String>,
    /// 생성 시간
    pub created_at: DateTime<Utc>,
}

impl ConstraintDefinition {
    /// 새 제약조건 정의 생성
    pub fn new(
        name: impl Into<String>,
        constraint_type: ConstraintType,
        target: ConstraintTarget,
        properties: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            constraint_type,
            target,
            properties,
            created_at: Utc::now(),
        }
    }

    /// UNIQUE 제약조건 생성 헬퍼
    pub fn unique(
        name: impl Into<String>,
        label: impl Into<String>,
        property: impl Into<String>,
    ) -> Self {
        Self::new(
            name,
            ConstraintType::Unique,
            ConstraintTarget::NodeLabel(label.into()),
            vec![property.into()],
        )
    }

    /// NOT NULL 제약조건 생성 헬퍼
    pub fn not_null(
        name: impl Into<String>,
        label: impl Into<String>,
        property: impl Into<String>,
    ) -> Self {
        Self::new(
            name,
            ConstraintType::NotNull,
            ConstraintTarget::NodeLabel(label.into()),
            vec![property.into()],
        )
    }

    /// NODE KEY 제약조건 생성 헬퍼
    pub fn node_key(
        name: impl Into<String>,
        label: impl Into<String>,
        properties: Vec<String>,
    ) -> Self {
        Self::new(
            name,
            ConstraintType::NodeKey,
            ConstraintTarget::NodeLabel(label.into()),
            properties,
        )
    }

    /// 대상 라벨 반환 (노드 제약조건인 경우)
    pub fn label(&self) -> Option<&str> {
        match &self.target {
            ConstraintTarget::NodeLabel(label) => Some(label),
            _ => None,
        }
    }

    /// 대상 관계 타입 반환 (관계 제약조건인 경우)
    pub fn relationship_type(&self) -> Option<&str> {
        match &self.target {
            ConstraintTarget::RelationshipType(rel_type) => Some(rel_type),
            _ => None,
        }
    }
}

/// UNIQUE 인덱스 (빠른 중복 검사용)
#[derive(Debug, Default)]
struct UniqueIndex {
    /// (label, property_names_key) -> { node_id -> property_values }
    /// property_names_key는 정렬된 프로퍼티 이름들의 조합
    indexes: HashMap<String, HashMap<Vec<Value>, NodeId>>,
}

impl UniqueIndex {
    fn new() -> Self {
        Self::default()
    }

    /// 인덱스 키 생성
    fn make_key(label: &str, properties: &[String]) -> String {
        let mut props = properties.to_vec();
        props.sort();
        format!("{}:{}", label, props.join(","))
    }

    /// 값 삽입 (중복 시 에러)
    fn insert(
        &mut self,
        label: &str,
        properties: &[String],
        values: Vec<Value>,
        node_id: NodeId,
    ) -> Result<()> {
        let key = Self::make_key(label, properties);
        let index = self.indexes.entry(key.clone()).or_default();

        if let Some(&existing_id) = index.get(&values)
            && existing_id != node_id
        {
            return Err(GraphError::Constraint {
                constraint_name: "validation".to_string(),
                message: format!(
                    "UNIQUE constraint violation: properties {:?} with values {:?} already exists on node {}",
                    properties, values, existing_id
                ),
            });
        }

        index.insert(values, node_id);
        Ok(())
    }

    /// 값 제거
    fn remove(&mut self, label: &str, properties: &[String], values: &[Value]) {
        let key = Self::make_key(label, properties);
        if let Some(index) = self.indexes.get_mut(&key) {
            index.remove(values);
        }
    }

    /// 값 존재 여부 확인 (자신 제외)
    fn contains_except(
        &self,
        label: &str,
        properties: &[String],
        values: &[Value],
        exclude_id: Option<NodeId>,
    ) -> bool {
        let key = Self::make_key(label, properties);
        if let Some(index) = self.indexes.get(&key)
            && let Some(&existing_id) = index.get(values)
        {
            if let Some(exclude) = exclude_id {
                return existing_id != exclude;
            }
            return true;
        }
        false
    }

    /// 특정 제약조건의 인덱스 초기화
    fn clear(&mut self, label: &str, properties: &[String]) {
        let key = Self::make_key(label, properties);
        self.indexes.remove(&key);
    }
}

/// 제약조건 관리자
pub struct ConstraintManager {
    /// 제약조건 저장소 (이름 -> 정의)
    constraints: RwLock<HashMap<String, ConstraintDefinition>>,
    /// UNIQUE 인덱스 (빠른 중복 검사용)
    unique_indexes: RwLock<UniqueIndex>,
}

impl Default for ConstraintManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstraintManager {
    /// 새 제약조건 관리자 생성
    pub fn new() -> Self {
        Self {
            constraints: RwLock::new(HashMap::new()),
            unique_indexes: RwLock::new(UniqueIndex::new()),
        }
    }

    /// 제약조건 추가
    pub fn add_constraint(&self, constraint: ConstraintDefinition) -> Result<()> {
        let mut constraints = write_lock(&self.constraints, "constraints")?;

        if constraints.contains_key(&constraint.name) {
            return Err(GraphError::Constraint {
                constraint_name: "validation".to_string(),
                message: format!("Constraint '{}' already exists", constraint.name),
            });
        }

        constraints.insert(constraint.name.clone(), constraint);
        Ok(())
    }

    /// 제약조건 제거
    pub fn remove_constraint(&self, name: &str) -> Result<ConstraintDefinition> {
        let mut constraints = write_lock(&self.constraints, "constraints")?;

        let constraint = constraints
            .remove(name)
            .ok_or_else(|| GraphError::Constraint {
                constraint_name: "validation".to_string(),
                message: format!("Constraint '{}' not found", name),
            })?;

        // UNIQUE 인덱스 정리
        if matches!(
            constraint.constraint_type,
            ConstraintType::Unique | ConstraintType::NodeKey
        ) && let Some(label) = constraint.label()
        {
            let mut indexes = write_lock(&self.unique_indexes, "unique_indexes")?;
            indexes.clear(label, &constraint.properties);
        }

        Ok(constraint)
    }

    /// 제약조건 조회
    pub fn get_constraint(&self, name: &str) -> Result<Option<ConstraintDefinition>> {
        let constraints = read_lock(&self.constraints, "constraints")?;

        Ok(constraints.get(name).cloned())
    }

    /// 모든 제약조건 목록
    pub fn list_constraints(&self) -> Result<Vec<ConstraintDefinition>> {
        let constraints = read_lock(&self.constraints, "constraints")?;

        Ok(constraints.values().cloned().collect())
    }

    /// 특정 라벨에 대한 제약조건 목록
    pub fn get_constraints_for_label(&self, label: &str) -> Result<Vec<ConstraintDefinition>> {
        let constraints = read_lock(&self.constraints, "constraints")?;

        Ok(constraints
            .values()
            .filter(|c| c.label() == Some(label))
            .cloned()
            .collect())
    }

    /// 특정 관계 타입에 대한 제약조건 목록
    pub fn get_constraints_for_relationship_type(
        &self,
        rel_type: &str,
    ) -> Result<Vec<ConstraintDefinition>> {
        let constraints = read_lock(&self.constraints, "constraints")?;

        Ok(constraints
            .values()
            .filter(|c| c.relationship_type() == Some(rel_type))
            .cloned()
            .collect())
    }

    /// NOT NULL 제약조건 검증
    pub fn validate_not_null(
        &self,
        constraint: &ConstraintDefinition,
        properties: &IndexMap<String, Value>,
    ) -> Result<()> {
        for prop_name in &constraint.properties {
            match properties.get(prop_name) {
                None | Some(Value::Null) => {
                    return Err(GraphError::Constraint {
                        constraint_name: "validation".to_string(),
                        message: format!(
                            "NOT NULL constraint '{}' violation: property '{}' cannot be null",
                            constraint.name, prop_name
                        ),
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// UNIQUE 제약조건 검증
    pub fn validate_unique(
        &self,
        constraint: &ConstraintDefinition,
        properties: &IndexMap<String, Value>,
        exclude_id: Option<NodeId>,
    ) -> Result<()> {
        let label = constraint.label().ok_or_else(|| GraphError::Constraint {
            constraint_name: "validation".to_string(),
            message: "UNIQUE constraint must target a node label".to_string(),
        })?;

        // 프로퍼티 값 추출
        let values: Vec<Value> = constraint
            .properties
            .iter()
            .map(|p| properties.get(p).cloned().unwrap_or(Value::Null))
            .collect();

        // 모두 NULL이면 UNIQUE 검사 스킵 (SQL 표준)
        if values.iter().all(|v| matches!(v, Value::Null)) {
            return Ok(());
        }

        let indexes = read_lock(&self.unique_indexes, "unique_indexes")?;

        if indexes.contains_except(label, &constraint.properties, &values, exclude_id) {
            return Err(GraphError::Constraint {
                constraint_name: "validation".to_string(),
                message: format!(
                    "UNIQUE constraint '{}' violation: properties {:?} with values {:?} already exists",
                    constraint.name, constraint.properties, values
                ),
            });
        }

        Ok(())
    }

    /// NODE KEY 제약조건 검증 (NOT NULL + UNIQUE)
    pub fn validate_node_key(
        &self,
        constraint: &ConstraintDefinition,
        properties: &IndexMap<String, Value>,
        exclude_id: Option<NodeId>,
    ) -> Result<()> {
        // 1. NOT NULL 검증
        self.validate_not_null(constraint, properties)?;

        // 2. UNIQUE 검증
        self.validate_unique(constraint, properties, exclude_id)?;

        Ok(())
    }

    /// 노드 생성 시 전체 제약조건 검증
    pub fn validate_node_create(
        &self,
        label: &str,
        properties: &IndexMap<String, Value>,
    ) -> Result<()> {
        let constraints = self.get_constraints_for_label(label)?;

        for constraint in constraints {
            match constraint.constraint_type {
                ConstraintType::NotNull => {
                    self.validate_not_null(&constraint, properties)?;
                }
                ConstraintType::Unique => {
                    self.validate_unique(&constraint, properties, None)?;
                }
                ConstraintType::NodeKey => {
                    self.validate_node_key(&constraint, properties, None)?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// 노드 수정 시 전체 제약조건 검증
    pub fn validate_node_update(
        &self,
        node_id: NodeId,
        label: &str,
        new_properties: &IndexMap<String, Value>,
    ) -> Result<()> {
        let constraints = self.get_constraints_for_label(label)?;

        for constraint in constraints {
            match constraint.constraint_type {
                ConstraintType::NotNull => {
                    self.validate_not_null(&constraint, new_properties)?;
                }
                ConstraintType::Unique => {
                    self.validate_unique(&constraint, new_properties, Some(node_id))?;
                }
                ConstraintType::NodeKey => {
                    self.validate_node_key(&constraint, new_properties, Some(node_id))?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// UNIQUE 인덱스에 값 등록 (노드 생성 후 호출)
    pub fn register_unique_value(
        &self,
        label: &str,
        properties: &IndexMap<String, Value>,
        node_id: NodeId,
    ) -> Result<()> {
        let constraints = self.get_constraints_for_label(label)?;

        let mut indexes = write_lock(&self.unique_indexes, "unique_indexes")?;

        for constraint in constraints {
            if matches!(
                constraint.constraint_type,
                ConstraintType::Unique | ConstraintType::NodeKey
            ) {
                let values: Vec<Value> = constraint
                    .properties
                    .iter()
                    .map(|p| properties.get(p).cloned().unwrap_or(Value::Null))
                    .collect();

                // NULL이 아닌 경우에만 인덱스에 등록
                if !values.iter().all(|v| matches!(v, Value::Null)) {
                    indexes.insert(label, &constraint.properties, values, node_id)?;
                }
            }
        }

        Ok(())
    }

    /// UNIQUE 인덱스에서 값 제거 (노드 삭제 시 호출)
    pub fn unregister_unique_value(
        &self,
        label: &str,
        properties: &IndexMap<String, Value>,
    ) -> Result<()> {
        let constraints = self.get_constraints_for_label(label)?;

        let mut indexes = write_lock(&self.unique_indexes, "unique_indexes")?;

        for constraint in constraints {
            if matches!(
                constraint.constraint_type,
                ConstraintType::Unique | ConstraintType::NodeKey
            ) {
                let values: Vec<Value> = constraint
                    .properties
                    .iter()
                    .map(|p| properties.get(p).cloned().unwrap_or(Value::Null))
                    .collect();

                indexes.remove(label, &constraint.properties, &values);
            }
        }

        Ok(())
    }

    /// JSON으로 직렬화
    pub fn to_json(&self) -> Result<String> {
        let constraints = read_lock(&self.constraints, "constraints")?;

        let list: Vec<&ConstraintDefinition> = constraints.values().collect();
        serde_json::to_string_pretty(&list).map_err(|e| GraphError::Serialization {
            message: e.to_string(),
            source: None,
        })
    }

    /// JSON에서 역직렬화
    pub fn from_json(json: &str) -> Result<Self> {
        let list: Vec<ConstraintDefinition> =
            serde_json::from_str(json).map_err(|e| GraphError::Serialization {
                message: e.to_string(),
                source: None,
            })?;

        let manager = Self::new();
        for constraint in list {
            manager.add_constraint(constraint)?;
        }

        Ok(manager)
    }

    /// 제약조건 수
    pub fn count(&self) -> usize {
        self.constraints.read().map(|c| c.len()).unwrap_or(0)
    }
}
