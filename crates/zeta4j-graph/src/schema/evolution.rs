// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 스키마 진화 관리 (Schema Evolution)
//!
//! 스키마 버전 관리 및 마이그레이션 시스템입니다.
//!
//! # 주요 기능
//!
//! - 스키마 버전 관리 (SchemaVersion)
//! - 마이그레이션 정의 및 실행 (SchemaMigration)
//! - 호환성 검사 (CompatibilityLevel)
//! - 롤백 지원
//!
//! # 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::schema::evolution::{SchemaEvolutionManager, SchemaMigration, MigrationOp};
//!
//! let mut evo_manager = SchemaEvolutionManager::new();
//!
//! // 마이그레이션 정의
//! let migration = SchemaMigration::new("1.0.0", "1.1.0")
//!     .with_op(MigrationOp::add_property("Person", "email", PropertyType::String))
//!     .with_description("Add email property to Person");
//!
//! evo_manager.register_migration(migration)?;
//! evo_manager.apply_pending(&mut schema_manager)?;
//! ```

use super::{
    NodeLabelSchema, PropertyDefinition, PropertyType, RelationshipTypeSchema, SchemaManager,
};
use crate::core::Value;
use crate::error::{GraphError, Result};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// 스키마 버전
// ============================================================================

/// 스키마 버전
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaVersion {
    /// 주 버전 (비호환 변경)
    pub major: u32,
    /// 부 버전 (후방 호환 추가)
    pub minor: u32,
    /// 패치 버전 (버그 수정)
    pub patch: u32,
    /// 생성 시간 (Unix timestamp)
    pub timestamp: u64,
    /// 변경 설명
    pub description: String,
}

impl SchemaVersion {
    /// 새 버전 생성
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            major,
            minor,
            patch,
            timestamp,
            description: String::new(),
        }
    }

    /// 버전 문자열에서 파싱 (예: "1.2.3")
    pub fn parse(version_str: &str) -> Result<Self> {
        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() != 3 {
            return Err(GraphError::Schema(format!(
                "Invalid version format: '{}'. Expected 'major.minor.patch'",
                version_str
            )));
        }

        let major = parts[0]
            .parse::<u32>()
            .map_err(|_| GraphError::Schema(format!("Invalid major version: '{}'", parts[0])))?;
        let minor = parts[1]
            .parse::<u32>()
            .map_err(|_| GraphError::Schema(format!("Invalid minor version: '{}'", parts[1])))?;
        let patch = parts[2]
            .parse::<u32>()
            .map_err(|_| GraphError::Schema(format!("Invalid patch version: '{}'", parts[2])))?;

        Ok(Self::new(major, minor, patch))
    }

    /// 초기 버전 (0.0.0)
    pub fn initial() -> Self {
        Self {
            major: 0,
            minor: 0,
            patch: 0,
            timestamp: 0,
            description: "Initial version".to_string(),
        }
    }

    /// 설명 추가
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// 주 버전 증가
    pub fn bump_major(&self) -> Self {
        Self::new(self.major + 1, 0, 0)
    }

    /// 부 버전 증가
    pub fn bump_minor(&self) -> Self {
        Self::new(self.major, self.minor + 1, 0)
    }

    /// 패치 버전 증가
    pub fn bump_patch(&self) -> Self {
        Self::new(self.major, self.minor, self.patch + 1)
    }
}

impl std::fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl PartialOrd for SchemaVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SchemaVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.major.cmp(&other.major) {
            std::cmp::Ordering::Equal => match self.minor.cmp(&other.minor) {
                std::cmp::Ordering::Equal => self.patch.cmp(&other.patch),
                other => other,
            },
            other => other,
        }
    }
}

impl Default for SchemaVersion {
    fn default() -> Self {
        Self::initial()
    }
}

// ============================================================================
// 호환성 수준
// ============================================================================

/// 호환성 수준
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatibilityLevel {
    /// 완전 호환 (읽기/쓰기 모두 가능)
    Full,
    /// 전방 호환 (새 버전이 구 데이터 읽기 가능)
    Forward,
    /// 후방 호환 (구 버전이 새 데이터 읽기 가능)
    Backward,
    /// 비호환 (마이그레이션 필수)
    Breaking,
}

impl CompatibilityLevel {
    /// 호환 가능 여부
    pub fn is_compatible(&self) -> bool {
        !matches!(self, CompatibilityLevel::Breaking)
    }

    /// 두 호환성 수준 병합 (더 낮은 수준 선택)
    pub fn merge(&self, other: &CompatibilityLevel) -> CompatibilityLevel {
        match (self, other) {
            (CompatibilityLevel::Full, CompatibilityLevel::Full) => CompatibilityLevel::Full,
            (CompatibilityLevel::Breaking, _) | (_, CompatibilityLevel::Breaking) => {
                CompatibilityLevel::Breaking
            }
            (CompatibilityLevel::Forward, CompatibilityLevel::Backward)
            | (CompatibilityLevel::Backward, CompatibilityLevel::Forward) => {
                CompatibilityLevel::Breaking
            }
            (CompatibilityLevel::Forward, _) | (_, CompatibilityLevel::Forward) => {
                CompatibilityLevel::Forward
            }
            (CompatibilityLevel::Backward, _) | (_, CompatibilityLevel::Backward) => {
                CompatibilityLevel::Backward
            }
        }
    }
}

// ============================================================================
// 마이그레이션 대상
// ============================================================================

/// 마이그레이션 대상
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationTarget {
    /// 노드 라벨
    NodeLabel(String),
    /// 관계 타입
    RelationshipType(String),
}

// ============================================================================
// 타입 변환기
// ============================================================================

/// 타입 변환기
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeConverter {
    /// 문자열 → 정수
    StringToInteger,
    /// 정수 → 문자열
    IntegerToString,
    /// 정수 → 실수
    IntegerToFloat,
    /// 실수 → 정수 (반올림)
    FloatToInteger,
    /// 문자열 → 불리언
    StringToBoolean,
    /// 불리언 → 문자열
    BooleanToString,
}

impl TypeConverter {
    /// 값 변환
    pub fn convert(&self, value: &Value) -> Result<Value> {
        match self {
            TypeConverter::StringToInteger => {
                if let Value::String(s) = value {
                    s.parse::<i64>().map(Value::Integer).map_err(|_| {
                        GraphError::Schema(format!("Cannot convert '{}' to integer", s))
                    })
                } else {
                    Err(GraphError::Schema("Expected String value".to_string()))
                }
            }
            TypeConverter::IntegerToString => {
                if let Value::Integer(i) = value {
                    Ok(Value::String(i.to_string()))
                } else {
                    Err(GraphError::Schema("Expected Integer value".to_string()))
                }
            }
            TypeConverter::IntegerToFloat => {
                if let Value::Integer(i) = value {
                    Ok(Value::Float(*i as f64))
                } else {
                    Err(GraphError::Schema("Expected Integer value".to_string()))
                }
            }
            TypeConverter::FloatToInteger => {
                if let Value::Float(f) = value {
                    Ok(Value::Integer(f.round() as i64))
                } else {
                    Err(GraphError::Schema("Expected Float value".to_string()))
                }
            }
            TypeConverter::StringToBoolean => {
                if let Value::String(s) = value {
                    match s.to_lowercase().as_str() {
                        "true" | "yes" | "1" => Ok(Value::Boolean(true)),
                        "false" | "no" | "0" => Ok(Value::Boolean(false)),
                        _ => Err(GraphError::Schema(format!(
                            "Cannot convert '{}' to boolean",
                            s
                        ))),
                    }
                } else {
                    Err(GraphError::Schema("Expected String value".to_string()))
                }
            }
            TypeConverter::BooleanToString => {
                if let Value::Boolean(b) = value {
                    Ok(Value::String(b.to_string()))
                } else {
                    Err(GraphError::Schema("Expected Boolean value".to_string()))
                }
            }
        }
    }
}

// ============================================================================
// 마이그레이션 작업
// ============================================================================

/// 마이그레이션 작업
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigrationOp {
    /// 프로퍼티 추가
    AddProperty {
        target: MigrationTarget,
        name: String,
        prop_type: PropertyType,
        default: Option<Value>,
        required: bool,
    },
    /// 프로퍼티 제거
    RemoveProperty {
        target: MigrationTarget,
        name: String,
    },
    /// 프로퍼티 이름 변경
    RenameProperty {
        target: MigrationTarget,
        old_name: String,
        new_name: String,
    },
    /// 프로퍼티 타입 변경
    ChangePropertyType {
        target: MigrationTarget,
        name: String,
        new_type: PropertyType,
        converter: Option<TypeConverter>,
    },
    /// 노드 라벨 추가
    AddLabel {
        label: String,
        schema: NodeLabelSchema,
    },
    /// 노드 라벨 제거
    RemoveLabel { label: String },
    /// 관계 타입 추가
    AddRelationshipType {
        rel_type: String,
        schema: RelationshipTypeSchema,
    },
    /// 관계 타입 제거
    RemoveRelationshipType { rel_type: String },
}

impl MigrationOp {
    /// 노드 라벨에 프로퍼티 추가
    pub fn add_node_property(label: &str, name: &str, prop_type: PropertyType) -> Self {
        MigrationOp::AddProperty {
            target: MigrationTarget::NodeLabel(label.to_string()),
            name: name.to_string(),
            prop_type,
            default: None,
            required: false,
        }
    }

    /// 노드 라벨에 필수 프로퍼티 추가
    pub fn add_node_property_required(
        label: &str,
        name: &str,
        prop_type: PropertyType,
        default: Value,
    ) -> Self {
        MigrationOp::AddProperty {
            target: MigrationTarget::NodeLabel(label.to_string()),
            name: name.to_string(),
            prop_type,
            default: Some(default),
            required: true,
        }
    }

    /// 관계 타입에 프로퍼티 추가
    pub fn add_rel_property(rel_type: &str, name: &str, prop_type: PropertyType) -> Self {
        MigrationOp::AddProperty {
            target: MigrationTarget::RelationshipType(rel_type.to_string()),
            name: name.to_string(),
            prop_type,
            default: None,
            required: false,
        }
    }

    /// 노드 라벨에서 프로퍼티 제거
    pub fn remove_node_property(label: &str, name: &str) -> Self {
        MigrationOp::RemoveProperty {
            target: MigrationTarget::NodeLabel(label.to_string()),
            name: name.to_string(),
        }
    }

    /// 관계 타입에서 프로퍼티 제거
    pub fn remove_rel_property(rel_type: &str, name: &str) -> Self {
        MigrationOp::RemoveProperty {
            target: MigrationTarget::RelationshipType(rel_type.to_string()),
            name: name.to_string(),
        }
    }

    /// 프로퍼티 이름 변경
    pub fn rename_node_property(label: &str, old_name: &str, new_name: &str) -> Self {
        MigrationOp::RenameProperty {
            target: MigrationTarget::NodeLabel(label.to_string()),
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
        }
    }

    /// 프로퍼티 타입 변경
    pub fn change_node_property_type(
        label: &str,
        name: &str,
        new_type: PropertyType,
        converter: Option<TypeConverter>,
    ) -> Self {
        MigrationOp::ChangePropertyType {
            target: MigrationTarget::NodeLabel(label.to_string()),
            name: name.to_string(),
            new_type,
            converter,
        }
    }

    /// 노드 라벨 추가
    pub fn add_label(schema: NodeLabelSchema) -> Self {
        MigrationOp::AddLabel {
            label: schema.label.clone(),
            schema,
        }
    }

    /// 노드 라벨 제거
    pub fn remove_label(label: &str) -> Self {
        MigrationOp::RemoveLabel {
            label: label.to_string(),
        }
    }

    /// 관계 타입 추가
    pub fn add_relationship_type(schema: RelationshipTypeSchema) -> Self {
        MigrationOp::AddRelationshipType {
            rel_type: schema.relationship_type.clone(),
            schema,
        }
    }

    /// 관계 타입 제거
    pub fn remove_relationship_type(rel_type: &str) -> Self {
        MigrationOp::RemoveRelationshipType {
            rel_type: rel_type.to_string(),
        }
    }

    /// 호환성 수준 결정
    pub fn compatibility(&self) -> CompatibilityLevel {
        match self {
            MigrationOp::AddProperty { required, .. } => {
                if *required {
                    CompatibilityLevel::Backward
                } else {
                    CompatibilityLevel::Full
                }
            }
            MigrationOp::RemoveProperty { .. } => CompatibilityLevel::Forward,
            MigrationOp::RenameProperty { .. } => CompatibilityLevel::Breaking,
            MigrationOp::ChangePropertyType { converter, .. } => {
                if converter.is_some() {
                    CompatibilityLevel::Forward
                } else {
                    CompatibilityLevel::Breaking
                }
            }
            MigrationOp::AddLabel { .. } | MigrationOp::AddRelationshipType { .. } => {
                CompatibilityLevel::Full
            }
            MigrationOp::RemoveLabel { .. } | MigrationOp::RemoveRelationshipType { .. } => {
                CompatibilityLevel::Forward
            }
        }
    }
}

// ============================================================================
// 스키마 마이그레이션
// ============================================================================

/// 스키마 마이그레이션
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaMigration {
    /// 시작 버전
    pub from_version: SchemaVersion,
    /// 대상 버전
    pub to_version: SchemaVersion,
    /// 마이그레이션 작업들
    pub operations: Vec<MigrationOp>,
    /// 설명
    pub description: String,
    /// 롤백 가능 여부
    pub reversible: bool,
}

impl SchemaMigration {
    /// 새 마이그레이션 생성
    pub fn new(from: &str, to: &str) -> Result<Self> {
        let from_version = SchemaVersion::parse(from)?;
        let to_version = SchemaVersion::parse(to)?;

        if to_version <= from_version {
            return Err(GraphError::Schema(format!(
                "Target version {} must be greater than source version {}",
                to, from
            )));
        }

        Ok(Self {
            from_version,
            to_version,
            operations: Vec::new(),
            description: String::new(),
            reversible: true,
        })
    }

    /// 버전 객체로 마이그레이션 생성
    pub fn with_versions(from: SchemaVersion, to: SchemaVersion) -> Result<Self> {
        if to <= from {
            return Err(GraphError::Schema(format!(
                "Target version {} must be greater than source version {}",
                to, from
            )));
        }

        Ok(Self {
            from_version: from,
            to_version: to,
            operations: Vec::new(),
            description: String::new(),
            reversible: true,
        })
    }

    /// 작업 추가
    pub fn with_op(mut self, op: MigrationOp) -> Self {
        // RenameProperty는 되돌릴 수 없음
        if matches!(op, MigrationOp::RenameProperty { .. }) {
            self.reversible = false;
        }
        self.operations.push(op);
        self
    }

    /// 설명 추가
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// 전체 호환성 수준 계산
    pub fn compatibility(&self) -> CompatibilityLevel {
        if self.operations.is_empty() {
            return CompatibilityLevel::Full;
        }

        self.operations
            .iter()
            .map(|op| op.compatibility())
            .fold(CompatibilityLevel::Full, |acc, level| acc.merge(&level))
    }

    /// 롤백 가능 여부
    pub fn is_reversible(&self) -> bool {
        self.reversible
    }
}

// ============================================================================
// 마이그레이션 결과
// ============================================================================

/// 마이그레이션 결과
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// 시작 버전
    pub from_version: SchemaVersion,
    /// 대상 버전
    pub to_version: SchemaVersion,
    /// 적용된 작업 수
    pub operations_applied: usize,
    /// 소요 시간 (ms)
    pub elapsed_ms: u64,
    /// 경고 메시지
    pub warnings: Vec<String>,
}

/// 드라이런 결과
#[derive(Debug, Clone)]
pub struct DryRunResult {
    /// 호환성 수준
    pub compatibility: CompatibilityLevel,
    /// 영향받는 항목
    pub affected: AffectedItems,
    /// 경고 메시지
    pub warnings: Vec<String>,
    /// 오류 메시지
    pub errors: Vec<String>,
}

/// 영향받는 항목
#[derive(Debug, Clone, Default)]
pub struct AffectedItems {
    /// 영향받는 노드 라벨
    pub node_labels: Vec<String>,
    /// 영향받는 관계 타입
    pub relationship_types: Vec<String>,
    /// 프로퍼티 변경 수
    pub property_changes: usize,
}

// ============================================================================
// 스키마 진화 관리자
// ============================================================================

/// 스키마 진화 관리자
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchemaEvolutionManager {
    /// 현재 버전
    current_version: SchemaVersion,
    /// 버전 히스토리
    version_history: Vec<SchemaVersion>,
    /// 등록된 마이그레이션
    migrations: Vec<SchemaMigration>,
    /// 적용 대기 중인 마이그레이션
    #[serde(skip)]
    pending_migrations: Vec<SchemaMigration>,
}

impl SchemaEvolutionManager {
    /// 새 진화 관리자 생성
    pub fn new() -> Self {
        Self {
            current_version: SchemaVersion::initial(),
            version_history: vec![SchemaVersion::initial()],
            migrations: Vec::new(),
            pending_migrations: Vec::new(),
        }
    }

    /// 버전 지정하여 생성
    pub fn with_version(version: SchemaVersion) -> Self {
        Self {
            current_version: version.clone(),
            version_history: vec![version],
            migrations: Vec::new(),
            pending_migrations: Vec::new(),
        }
    }

    /// 현재 버전 반환
    pub fn current_version(&self) -> &SchemaVersion {
        &self.current_version
    }

    /// 버전 히스토리 반환
    pub fn version_history(&self) -> &[SchemaVersion] {
        &self.version_history
    }

    /// 등록된 마이그레이션 목록
    pub fn migrations(&self) -> &[SchemaMigration] {
        &self.migrations
    }

    /// 대기 중인 마이그레이션 수
    pub fn pending_count(&self) -> usize {
        self.pending_migrations.len()
    }

    /// 마이그레이션 등록
    pub fn register_migration(&mut self, migration: SchemaMigration) -> Result<()> {
        // 버전 체인 검증 (버전 번호만 비교, timestamp 무시)
        let current_version_str = self.current_version.to_string();
        let from_version_str = migration.from_version.to_string();

        if from_version_str != current_version_str {
            // 기존 마이그레이션에서 연결되는지 확인
            let last_to_version = self
                .pending_migrations
                .last()
                .map(|m| m.to_version.to_string())
                .unwrap_or_else(|| current_version_str.clone());

            if from_version_str != last_to_version {
                return Err(GraphError::Schema(format!(
                    "Migration chain broken: expected from_version {}, got {}",
                    last_to_version, from_version_str
                )));
            }
        }

        self.pending_migrations.push(migration);
        Ok(())
    }

    /// 마이그레이션 검증
    pub fn validate_migration(&self, migration: &SchemaMigration) -> Result<Vec<String>> {
        let mut warnings = Vec::new();

        // 호환성 경고
        let compat = migration.compatibility();
        if compat == CompatibilityLevel::Breaking {
            warnings.push("This migration contains breaking changes".to_string());
        }

        // 비가역 작업 경고
        if !migration.is_reversible() {
            warnings.push("This migration cannot be rolled back".to_string());
        }

        // 작업별 검증
        for op in &migration.operations {
            match op {
                MigrationOp::AddProperty {
                    required: true,
                    default: None,
                    name,
                    ..
                } => {
                    return Err(GraphError::Schema(format!(
                        "Required property '{}' must have a default value",
                        name
                    )));
                }
                MigrationOp::ChangePropertyType {
                    converter: None,
                    name,
                    ..
                } => {
                    warnings.push(format!(
                        "Property '{}' type change without converter may cause data loss",
                        name
                    ));
                }
                _ => {}
            }
        }

        Ok(warnings)
    }

    /// 드라이런 (실제 적용 없이 결과 예측)
    pub fn dry_run(&self, migration: &SchemaMigration) -> DryRunResult {
        let mut affected = AffectedItems::default();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();

        for op in &migration.operations {
            match op {
                MigrationOp::AddProperty { target, .. }
                | MigrationOp::RemoveProperty { target, .. }
                | MigrationOp::RenameProperty { target, .. }
                | MigrationOp::ChangePropertyType { target, .. } => {
                    match target {
                        MigrationTarget::NodeLabel(label) => {
                            if !affected.node_labels.contains(label) {
                                affected.node_labels.push(label.clone());
                            }
                        }
                        MigrationTarget::RelationshipType(rel) => {
                            if !affected.relationship_types.contains(rel) {
                                affected.relationship_types.push(rel.clone());
                            }
                        }
                    }
                    affected.property_changes += 1;
                }
                MigrationOp::AddLabel { label, .. } | MigrationOp::RemoveLabel { label } => {
                    if !affected.node_labels.contains(label) {
                        affected.node_labels.push(label.clone());
                    }
                }
                MigrationOp::AddRelationshipType { rel_type, .. }
                | MigrationOp::RemoveRelationshipType { rel_type } => {
                    if !affected.relationship_types.contains(rel_type) {
                        affected.relationship_types.push(rel_type.clone());
                    }
                }
            }
        }

        // 검증
        match self.validate_migration(migration) {
            Ok(w) => warnings = w,
            Err(e) => errors.push(e.to_string()),
        }

        DryRunResult {
            compatibility: migration.compatibility(),
            affected,
            warnings,
            errors,
        }
    }

    /// 마이그레이션 적용
    pub fn apply_migration(
        &mut self,
        migration: &SchemaMigration,
        schema_manager: &mut SchemaManager,
    ) -> Result<MigrationResult> {
        let start = std::time::Instant::now();

        // 검증
        let warnings = self.validate_migration(migration)?;

        // 작업 적용
        let mut applied = 0;
        for op in &migration.operations {
            self.apply_operation(op, schema_manager)?;
            applied += 1;
        }

        // 버전 업데이트
        self.current_version = migration.to_version.clone();
        self.version_history.push(migration.to_version.clone());
        self.migrations.push(migration.clone());

        Ok(MigrationResult {
            from_version: migration.from_version.clone(),
            to_version: migration.to_version.clone(),
            operations_applied: applied,
            elapsed_ms: start.elapsed().as_millis() as u64,
            warnings,
        })
    }

    /// 대기 중인 모든 마이그레이션 적용
    pub fn apply_pending(
        &mut self,
        schema_manager: &mut SchemaManager,
    ) -> Result<Vec<MigrationResult>> {
        let pending = std::mem::take(&mut self.pending_migrations);
        let mut results = Vec::new();

        for migration in pending {
            let result = self.apply_migration(&migration, schema_manager)?;
            results.push(result);
        }

        Ok(results)
    }

    /// 마지막 마이그레이션 롤백
    pub fn rollback(&mut self, schema_manager: &mut SchemaManager) -> Result<()> {
        let migration = self
            .migrations
            .pop()
            .ok_or_else(|| GraphError::Schema("No migration to rollback".to_string()))?;

        if !migration.is_reversible() {
            self.migrations.push(migration);
            return Err(GraphError::Schema(
                "Last migration is not reversible".to_string(),
            ));
        }

        // 역순으로 작업 되돌리기
        for op in migration.operations.iter().rev() {
            self.rollback_operation(op, schema_manager)?;
        }

        // 버전 복원
        self.version_history.pop();
        self.current_version = self
            .version_history
            .last()
            .cloned()
            .unwrap_or_else(SchemaVersion::initial);

        Ok(())
    }

    /// 특정 버전까지 롤백
    pub fn rollback_to_version(
        &mut self,
        target: &SchemaVersion,
        schema_manager: &mut SchemaManager,
    ) -> Result<usize> {
        if *target >= self.current_version {
            return Err(GraphError::Schema(format!(
                "Target version {} must be less than current version {}",
                target, self.current_version
            )));
        }

        let mut rollback_count = 0;
        while self.current_version > *target {
            self.rollback(schema_manager)?;
            rollback_count += 1;
        }

        Ok(rollback_count)
    }

    // 작업 적용
    fn apply_operation(&self, op: &MigrationOp, schema_manager: &mut SchemaManager) -> Result<()> {
        match op {
            MigrationOp::AddProperty {
                target,
                name,
                prop_type,
                default,
                required,
            } => {
                let mut prop_def = PropertyDefinition::new(name, prop_type.clone());
                if *required {
                    prop_def = prop_def.not_null();
                }
                if let Some(def) = default {
                    prop_def = prop_def.with_default(def.clone());
                }

                match target {
                    MigrationTarget::NodeLabel(label) => {
                        if let Some(schema) = schema_manager.get_node_label_schema(label) {
                            let mut new_schema = schema.clone();
                            new_schema.properties.insert(name.clone(), prop_def);
                            schema_manager.drop_node_label_schema(label)?;
                            schema_manager.define_node_label(new_schema)?;
                        }
                    }
                    MigrationTarget::RelationshipType(rel_type) => {
                        if let Some(schema) = schema_manager.get_relationship_type_schema(rel_type)
                        {
                            let mut new_schema = schema.clone();
                            new_schema.properties.insert(name.clone(), prop_def);
                            schema_manager.drop_relationship_type_schema(rel_type)?;
                            schema_manager.define_relationship_type(new_schema)?;
                        }
                    }
                }
            }
            MigrationOp::RemoveProperty { target, name } => match target {
                MigrationTarget::NodeLabel(label) => {
                    if let Some(schema) = schema_manager.get_node_label_schema(label) {
                        let mut new_schema = schema.clone();
                        new_schema.properties.remove(name);
                        schema_manager.drop_node_label_schema(label)?;
                        schema_manager.define_node_label(new_schema)?;
                    }
                }
                MigrationTarget::RelationshipType(rel_type) => {
                    if let Some(schema) = schema_manager.get_relationship_type_schema(rel_type) {
                        let mut new_schema = schema.clone();
                        new_schema.properties.remove(name);
                        schema_manager.drop_relationship_type_schema(rel_type)?;
                        schema_manager.define_relationship_type(new_schema)?;
                    }
                }
            },
            MigrationOp::RenameProperty {
                target,
                old_name,
                new_name,
            } => match target {
                MigrationTarget::NodeLabel(label) => {
                    if let Some(schema) = schema_manager.get_node_label_schema(label) {
                        let mut new_schema = schema.clone();
                        if let Some(prop_def) = new_schema.properties.remove(old_name) {
                            let mut renamed = prop_def;
                            renamed.name = new_name.clone();
                            new_schema.properties.insert(new_name.clone(), renamed);
                        }
                        schema_manager.drop_node_label_schema(label)?;
                        schema_manager.define_node_label(new_schema)?;
                    }
                }
                MigrationTarget::RelationshipType(rel_type) => {
                    if let Some(schema) = schema_manager.get_relationship_type_schema(rel_type) {
                        let mut new_schema = schema.clone();
                        if let Some(prop_def) = new_schema.properties.remove(old_name) {
                            let mut renamed = prop_def;
                            renamed.name = new_name.clone();
                            new_schema.properties.insert(new_name.clone(), renamed);
                        }
                        schema_manager.drop_relationship_type_schema(rel_type)?;
                        schema_manager.define_relationship_type(new_schema)?;
                    }
                }
            },
            MigrationOp::ChangePropertyType {
                target,
                name,
                new_type,
                ..
            } => match target {
                MigrationTarget::NodeLabel(label) => {
                    if let Some(schema) = schema_manager.get_node_label_schema(label) {
                        let mut new_schema = schema.clone();
                        if let Some(prop_def) = new_schema.properties.get_mut(name) {
                            prop_def.property_type = new_type.clone();
                        }
                        schema_manager.drop_node_label_schema(label)?;
                        schema_manager.define_node_label(new_schema)?;
                    }
                }
                MigrationTarget::RelationshipType(rel_type) => {
                    if let Some(schema) = schema_manager.get_relationship_type_schema(rel_type) {
                        let mut new_schema = schema.clone();
                        if let Some(prop_def) = new_schema.properties.get_mut(name) {
                            prop_def.property_type = new_type.clone();
                        }
                        schema_manager.drop_relationship_type_schema(rel_type)?;
                        schema_manager.define_relationship_type(new_schema)?;
                    }
                }
            },
            MigrationOp::AddLabel { schema, .. } => {
                schema_manager.define_node_label(schema.clone())?;
            }
            MigrationOp::RemoveLabel { label } => {
                schema_manager.drop_node_label_schema(label)?;
            }
            MigrationOp::AddRelationshipType { schema, .. } => {
                schema_manager.define_relationship_type(schema.clone())?;
            }
            MigrationOp::RemoveRelationshipType { rel_type } => {
                schema_manager.drop_relationship_type_schema(rel_type)?;
            }
        }
        Ok(())
    }

    // 작업 롤백
    fn rollback_operation(
        &self,
        op: &MigrationOp,
        schema_manager: &mut SchemaManager,
    ) -> Result<()> {
        match op {
            MigrationOp::AddProperty { target, name, .. } => {
                // 추가된 프로퍼티 제거
                self.apply_operation(
                    &MigrationOp::RemoveProperty {
                        target: target.clone(),
                        name: name.clone(),
                    },
                    schema_manager,
                )
            }
            MigrationOp::RemoveProperty { .. } => {
                // 제거된 프로퍼티는 복원 불가 (원래 정의를 알 수 없음)
                Err(GraphError::Schema(
                    "Cannot rollback RemoveProperty: original definition unknown".to_string(),
                ))
            }
            MigrationOp::RenameProperty { .. } => Err(GraphError::Schema(
                "Cannot rollback RenameProperty".to_string(),
            )),
            MigrationOp::ChangePropertyType { .. } => Err(GraphError::Schema(
                "Cannot rollback ChangePropertyType: original type unknown".to_string(),
            )),
            MigrationOp::AddLabel { label, .. } => schema_manager.drop_node_label_schema(label),
            MigrationOp::RemoveLabel { .. } => Err(GraphError::Schema(
                "Cannot rollback RemoveLabel: original schema unknown".to_string(),
            )),
            MigrationOp::AddRelationshipType { rel_type, .. } => {
                schema_manager.drop_relationship_type_schema(rel_type)
            }
            MigrationOp::RemoveRelationshipType { .. } => Err(GraphError::Schema(
                "Cannot rollback RemoveRelationshipType: original schema unknown".to_string(),
            )),
        }
    }
}

// ============================================================================
// 단위 테스트
