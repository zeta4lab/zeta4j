// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Schema Manager
//!
//! 인덱스 및 제약조건 레지스트리 관리
//!
//! 기능:
//! - 인덱스 정의 관리 (생성, 조회, 삭제)
//! - 제약조건 정의 관리 (생성, 조회, 삭제)
//! - 인덱스 상태 추적

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::error::{SystemError, SystemResult};
use super::storage::{SystemStorage, cf};

// ============================================================================
// Index Types
// ============================================================================

/// 인덱스 정의
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDef {
    /// 인덱스 이름
    pub name: String,
    /// 데이터베이스 이름
    pub database: String,
    /// 인덱스 타입
    pub index_type: IndexType,
    /// 엔티티 타입 (Node/Relationship)
    pub entity_type: EntityType,
    /// 라벨 또는 관계 타입
    pub label_or_type: String,
    /// 프로퍼티 목록
    pub properties: Vec<String>,
    /// 인덱스 설정
    pub config: IndexConfig,
    /// 인덱스 상태
    pub state: IndexState,
    /// 생성 시간
    pub created_at: DateTime<Utc>,
    /// 수정 시간
    pub updated_at: DateTime<Utc>,
    /// 인덱싱 진행률 (0.0 ~ 1.0)
    pub population_percent: f32,
}

impl IndexDef {
    /// 새 인덱스 정의 생성
    pub fn new(
        name: impl Into<String>,
        database: impl Into<String>,
        index_type: IndexType,
        entity_type: EntityType,
        label_or_type: impl Into<String>,
        properties: Vec<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            database: database.into(),
            index_type,
            entity_type,
            label_or_type: label_or_type.into(),
            properties,
            config: IndexConfig::default(),
            state: IndexState::Creating,
            created_at: now,
            updated_at: now,
            population_percent: 0.0,
        }
    }

    /// BTree 인덱스 생성
    pub fn btree(
        name: impl Into<String>,
        database: impl Into<String>,
        label: impl Into<String>,
        property: impl Into<String>,
    ) -> Self {
        Self::new(
            name,
            database,
            IndexType::BTree,
            EntityType::Node,
            label,
            vec![property.into()],
        )
    }

    /// 온라인 여부
    pub fn is_online(&self) -> bool {
        matches!(self.state, IndexState::Online)
    }
}

/// 인덱스 타입
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IndexType {
    /// B+Tree 인덱스 (범위 검색)
    BTree,
    /// Range 인덱스 (숫자 범위)
    Range,
    /// Text 인덱스 (텍스트 매칭)
    Text,
    /// Point 인덱스 (공간 검색)
    Point,
    /// Fulltext 인덱스 (전문 검색)
    Fulltext {
        analyzer: String,
        eventually_consistent: bool,
    },
    /// Vector 인덱스 (벡터 유사도 검색)
    Vector {
        dimensions: usize,
        similarity: Similarity,
        m: usize,
        ef_construction: usize,
    },
    /// Lookup 인덱스 (라벨/타입 토큰 조회)
    Lookup,
}

impl std::fmt::Display for IndexType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BTree => write!(f, "BTREE"),
            Self::Range => write!(f, "RANGE"),
            Self::Text => write!(f, "TEXT"),
            Self::Point => write!(f, "POINT"),
            Self::Fulltext { .. } => write!(f, "FULLTEXT"),
            Self::Vector { .. } => write!(f, "VECTOR"),
            Self::Lookup => write!(f, "LOOKUP"),
        }
    }
}

/// 벡터 유사도 함수
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Similarity {
    Cosine,
    Euclidean,
    DotProduct,
}

impl std::fmt::Display for Similarity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cosine => write!(f, "cosine"),
            Self::Euclidean => write!(f, "euclidean"),
            Self::DotProduct => write!(f, "dot_product"),
        }
    }
}

/// 엔티티 타입
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EntityType {
    Node,
    Relationship,
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Node => write!(f, "NODE"),
            Self::Relationship => write!(f, "RELATIONSHIP"),
        }
    }
}

/// 인덱스 설정
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexConfig {
    /// 공간 참조 시스템 (Point 인덱스용)
    pub spatial_crs: Option<String>,
    /// 최소/최대 범위 (Point 인덱스용)
    pub spatial_min: Option<Vec<f64>>,
    pub spatial_max: Option<Vec<f64>>,
}

/// 인덱스 상태
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IndexState {
    /// 생성 중
    Creating,
    /// 데이터 입력 중
    Populating,
    /// 온라인 (사용 가능)
    Online,
    /// 실패
    Failed { message: String },
    /// 삭제 중
    Dropping,
}

impl std::fmt::Display for IndexState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creating => write!(f, "CREATING"),
            Self::Populating => write!(f, "POPULATING"),
            Self::Online => write!(f, "ONLINE"),
            Self::Failed { message } => write!(f, "FAILED: {}", message),
            Self::Dropping => write!(f, "DROPPING"),
        }
    }
}

// ============================================================================
// Constraint Types
// ============================================================================

/// 제약조건 정의
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintDef {
    /// 제약조건 이름
    pub name: String,
    /// 데이터베이스 이름
    pub database: String,
    /// 제약조건 타입
    pub constraint_type: ConstraintType,
    /// 엔티티 타입 (Node/Relationship)
    pub entity_type: EntityType,
    /// 라벨 또는 관계 타입
    pub label_or_type: String,
    /// 프로퍼티 목록
    pub properties: Vec<String>,
    /// 생성 시간
    pub created_at: DateTime<Utc>,
}

impl ConstraintDef {
    /// 새 제약조건 생성
    pub fn new(
        name: impl Into<String>,
        database: impl Into<String>,
        constraint_type: ConstraintType,
        entity_type: EntityType,
        label_or_type: impl Into<String>,
        properties: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            database: database.into(),
            constraint_type,
            entity_type,
            label_or_type: label_or_type.into(),
            properties,
            created_at: Utc::now(),
        }
    }

    /// UNIQUE 제약조건 생성
    pub fn unique(
        name: impl Into<String>,
        database: impl Into<String>,
        label: impl Into<String>,
        property: impl Into<String>,
    ) -> Self {
        Self::new(
            name,
            database,
            ConstraintType::Unique,
            EntityType::Node,
            label,
            vec![property.into()],
        )
    }

    /// EXISTS (NOT NULL) 제약조건 생성
    pub fn exists(
        name: impl Into<String>,
        database: impl Into<String>,
        label: impl Into<String>,
        property: impl Into<String>,
    ) -> Self {
        Self::new(
            name,
            database,
            ConstraintType::Exists,
            EntityType::Node,
            label,
            vec![property.into()],
        )
    }

    /// NODE KEY 제약조건 생성
    pub fn node_key(
        name: impl Into<String>,
        database: impl Into<String>,
        label: impl Into<String>,
        properties: Vec<String>,
    ) -> Self {
        Self::new(
            name,
            database,
            ConstraintType::NodeKey,
            EntityType::Node,
            label,
            properties,
        )
    }
}

/// 제약조건 타입
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConstraintType {
    /// UNIQUE 제약조건
    Unique,
    /// EXISTS (NOT NULL) 제약조건
    Exists,
    /// NODE KEY (UNIQUE + EXISTS)
    NodeKey,
    /// RELATIONSHIP KEY
    RelationshipKey,
    /// 타입 제약조건
    PropertyType { value_type: ValueType },
}

impl std::fmt::Display for ConstraintType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unique => write!(f, "UNIQUE"),
            Self::Exists => write!(f, "EXISTS"),
            Self::NodeKey => write!(f, "NODE KEY"),
            Self::RelationshipKey => write!(f, "RELATIONSHIP KEY"),
            Self::PropertyType { value_type } => write!(f, "IS :: {}", value_type),
        }
    }
}

/// 프로퍼티 값 타입
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ValueType {
    Boolean,
    Integer,
    Float,
    String,
    Date,
    DateTime,
    Duration,
    Point,
    List,
    Map,
}

impl std::fmt::Display for ValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boolean => write!(f, "BOOLEAN"),
            Self::Integer => write!(f, "INTEGER"),
            Self::Float => write!(f, "FLOAT"),
            Self::String => write!(f, "STRING"),
            Self::Date => write!(f, "DATE"),
            Self::DateTime => write!(f, "DATETIME"),
            Self::Duration => write!(f, "DURATION"),
            Self::Point => write!(f, "POINT"),
            Self::List => write!(f, "LIST"),
            Self::Map => write!(f, "MAP"),
        }
    }
}

// ============================================================================
// Schema Manager
// ============================================================================

/// 스키마 매니저
pub struct SchemaManager {
    storage: Arc<SystemStorage>,
}

impl SchemaManager {
    /// 새 스키마 매니저 생성
    pub fn new(storage: Arc<SystemStorage>) -> Self {
        Self { storage }
    }

    // ========================================================================
    // Key Helpers
    // ========================================================================

    fn index_key(database: &str, name: &str) -> Vec<u8> {
        format!("idx:{}:{}", database, name).into_bytes()
    }

    fn constraint_key(database: &str, name: &str) -> Vec<u8> {
        format!("cstr:{}:{}", database, name).into_bytes()
    }

    fn index_prefix(database: &str) -> Vec<u8> {
        format!("idx:{}:", database).into_bytes()
    }

    fn constraint_prefix(database: &str) -> Vec<u8> {
        format!("cstr:{}:", database).into_bytes()
    }

    // ========================================================================
    // Index Management
    // ========================================================================

    /// 인덱스 생성
    pub fn create_index(&self, def: IndexDef) -> SystemResult<()> {
        let key = Self::index_key(&def.database, &def.name);

        // 중복 확인
        if self.storage.exists(cf::INDEXES, &key)? {
            return Err(SystemError::IndexAlreadyExists(def.name));
        }

        self.storage.put(cf::INDEXES, &key, &def)?;
        info!("Created index: {}.{}", def.database, def.name);

        Ok(())
    }

    /// 인덱스 조회
    pub fn get_index(&self, database: &str, name: &str) -> SystemResult<Option<IndexDef>> {
        let key = Self::index_key(database, name);
        self.storage.get(cf::INDEXES, &key)
    }

    /// 데이터베이스의 인덱스 목록
    pub fn list_indexes(&self, database: &str) -> SystemResult<Vec<IndexDef>> {
        let prefix = Self::index_prefix(database);
        let entries: Vec<(Vec<u8>, IndexDef)> = self.storage.scan_prefix(cf::INDEXES, &prefix)?;
        Ok(entries.into_iter().map(|(_, def)| def).collect())
    }

    /// 전체 인덱스 목록
    pub fn list_all_indexes(&self) -> SystemResult<Vec<IndexDef>> {
        let entries: Vec<(Vec<u8>, IndexDef)> = self.storage.scan_prefix(cf::INDEXES, b"idx:")?;
        Ok(entries.into_iter().map(|(_, def)| def).collect())
    }

    /// 인덱스 삭제
    pub fn drop_index(&self, database: &str, name: &str) -> SystemResult<()> {
        let key = Self::index_key(database, name);

        if !self.storage.exists(cf::INDEXES, &key)? {
            return Err(SystemError::IndexNotFound(name.to_string()));
        }

        self.storage.delete(cf::INDEXES, &key)?;
        info!("Dropped index: {}.{}", database, name);

        Ok(())
    }

    /// 인덱스 상태 변경
    pub fn set_index_state(
        &self,
        database: &str,
        name: &str,
        state: IndexState,
    ) -> SystemResult<()> {
        let key = Self::index_key(database, name);

        let mut def: IndexDef = self
            .storage
            .get(cf::INDEXES, &key)?
            .ok_or_else(|| SystemError::IndexNotFound(name.to_string()))?;

        def.state = state;
        def.updated_at = Utc::now();

        self.storage.put(cf::INDEXES, &key, &def)?;
        info!(
            "Index {}.{} state changed to: {}",
            database, name, def.state
        );

        Ok(())
    }

    /// 인덱스 진행률 업데이트
    pub fn set_index_progress(&self, database: &str, name: &str, percent: f32) -> SystemResult<()> {
        let key = Self::index_key(database, name);

        let mut def: IndexDef = self
            .storage
            .get(cf::INDEXES, &key)?
            .ok_or_else(|| SystemError::IndexNotFound(name.to_string()))?;

        def.population_percent = percent.clamp(0.0, 1.0);
        def.updated_at = Utc::now();

        self.storage.put(cf::INDEXES, &key, &def)?;

        Ok(())
    }

    /// 인덱스 존재 확인
    pub fn index_exists(&self, database: &str, name: &str) -> SystemResult<bool> {
        let key = Self::index_key(database, name);
        self.storage.exists(cf::INDEXES, &key)
    }

    /// 온라인 인덱스 목록
    pub fn list_online_indexes(&self, database: &str) -> SystemResult<Vec<IndexDef>> {
        let indexes = self.list_indexes(database)?;
        Ok(indexes.into_iter().filter(|i| i.is_online()).collect())
    }

    // ========================================================================
    // Constraint Management
    // ========================================================================

    /// 제약조건 생성
    pub fn create_constraint(&self, def: ConstraintDef) -> SystemResult<()> {
        let key = Self::constraint_key(&def.database, &def.name);

        // 중복 확인
        if self.storage.exists(cf::CONSTRAINTS, &key)? {
            return Err(SystemError::ConstraintAlreadyExists(def.name));
        }

        self.storage.put(cf::CONSTRAINTS, &key, &def)?;
        info!("Created constraint: {}.{}", def.database, def.name);

        Ok(())
    }

    /// 제약조건 조회
    pub fn get_constraint(
        &self,
        database: &str,
        name: &str,
    ) -> SystemResult<Option<ConstraintDef>> {
        let key = Self::constraint_key(database, name);
        self.storage.get(cf::CONSTRAINTS, &key)
    }

    /// 데이터베이스의 제약조건 목록
    pub fn list_constraints(&self, database: &str) -> SystemResult<Vec<ConstraintDef>> {
        let prefix = Self::constraint_prefix(database);
        let entries: Vec<(Vec<u8>, ConstraintDef)> =
            self.storage.scan_prefix(cf::CONSTRAINTS, &prefix)?;
        Ok(entries.into_iter().map(|(_, def)| def).collect())
    }

    /// 전체 제약조건 목록
    pub fn list_all_constraints(&self) -> SystemResult<Vec<ConstraintDef>> {
        let entries: Vec<(Vec<u8>, ConstraintDef)> =
            self.storage.scan_prefix(cf::CONSTRAINTS, b"cstr:")?;
        Ok(entries.into_iter().map(|(_, def)| def).collect())
    }

    /// 제약조건 삭제
    pub fn drop_constraint(&self, database: &str, name: &str) -> SystemResult<()> {
        let key = Self::constraint_key(database, name);

        if !self.storage.exists(cf::CONSTRAINTS, &key)? {
            return Err(SystemError::ConstraintNotFound(name.to_string()));
        }

        self.storage.delete(cf::CONSTRAINTS, &key)?;
        info!("Dropped constraint: {}.{}", database, name);

        Ok(())
    }

    /// 제약조건 존재 확인
    pub fn constraint_exists(&self, database: &str, name: &str) -> SystemResult<bool> {
        let key = Self::constraint_key(database, name);
        self.storage.exists(cf::CONSTRAINTS, &key)
    }

    // ========================================================================
    // Schema Query
    // ========================================================================

    /// 데이터베이스 스키마 조회
    pub fn get_schema(&self, database: &str) -> SystemResult<DatabaseSchema> {
        let indexes = self.list_indexes(database)?;
        let constraints = self.list_constraints(database)?;

        Ok(DatabaseSchema {
            database: database.to_string(),
            indexes,
            constraints,
        })
    }

    /// 라벨에 대한 인덱스 찾기
    pub fn find_indexes_for_label(
        &self,
        database: &str,
        label: &str,
    ) -> SystemResult<Vec<IndexDef>> {
        let indexes = self.list_indexes(database)?;
        Ok(indexes
            .into_iter()
            .filter(|i| i.entity_type == EntityType::Node && i.label_or_type == label)
            .collect())
    }

    /// 라벨과 프로퍼티에 대한 인덱스 찾기
    pub fn find_index(
        &self,
        database: &str,
        label: &str,
        property: &str,
    ) -> SystemResult<Option<IndexDef>> {
        let indexes = self.list_indexes(database)?;
        Ok(indexes.into_iter().find(|i| {
            i.entity_type == EntityType::Node
                && i.label_or_type == label
                && i.properties.len() == 1
                && i.properties[0] == property
                && i.is_online()
        }))
    }

    /// 라벨에 대한 제약조건 찾기
    pub fn find_constraints_for_label(
        &self,
        database: &str,
        label: &str,
    ) -> SystemResult<Vec<ConstraintDef>> {
        let constraints = self.list_constraints(database)?;
        Ok(constraints
            .into_iter()
            .filter(|c| c.entity_type == EntityType::Node && c.label_or_type == label)
            .collect())
    }

    /// UNIQUE 제약조건 찾기
    pub fn find_unique_constraint(
        &self,
        database: &str,
        label: &str,
        property: &str,
    ) -> SystemResult<Option<ConstraintDef>> {
        let constraints = self.list_constraints(database)?;
        Ok(constraints.into_iter().find(|c| {
            c.entity_type == EntityType::Node
                && c.label_or_type == label
                && c.properties.len() == 1
                && c.properties[0] == property
                && matches!(c.constraint_type, ConstraintType::Unique)
        }))
    }

    /// 인덱스 수
    pub fn index_count(&self, database: &str) -> SystemResult<usize> {
        Ok(self.list_indexes(database)?.len())
    }

    /// 제약조건 수
    pub fn constraint_count(&self, database: &str) -> SystemResult<usize> {
        Ok(self.list_constraints(database)?.len())
    }
}

/// 데이터베이스 스키마
#[derive(Debug, Clone)]
pub struct DatabaseSchema {
    pub database: String,
    pub indexes: Vec<IndexDef>,
    pub constraints: Vec<ConstraintDef>,
}
