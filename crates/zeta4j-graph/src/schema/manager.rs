// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 스키마 관리자
//!
//! 노드 라벨 스키마와 관계 타입 스키마를 관리합니다.
//!
//! # 스키마 변경 API (v1.43.0)
//!
//! 스키마 진화를 위한 편의 API를 제공합니다:
//!
//! - `alter_node_schema()`: 노드 라벨 스키마 변경
//! - `alter_rel_schema()`: 관계 타입 스키마 변경
//! - `migrate_schema()`: 버전 기반 마이그레이션
//!
//! # 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::schema::{SchemaManager, AlterSchemaOp, PropertyType, PropertyDefinition};
//!
//! let mut manager = SchemaManager::new();
//!
//! // 스키마 변경
//! manager.alter_node_schema("Person", |ops| {
//!     ops.add_property(PropertyDefinition::new("email", PropertyType::String));
//!     ops.remove_property("old_field");
//!     ops.rename_property("name", "full_name");
//! })?;
//!
//! // 버전 기반 마이그레이션
//! let results = manager.migrate_schema("1.0.0", "1.1.0", |migration| {
//!     migration.add_node_property("Person", "phone", PropertyType::String);
//! })?;
//! ```

use indexmap::IndexMap;

use super::common::ElementSchema;
use super::evolution::{
    MigrationOp, MigrationResult, SchemaEvolutionManager, SchemaMigration, TypeConverter,
};
use super::node_schema::NodeLabelSchema;
use super::rel_schema::RelationshipTypeSchema;
use super::types::{PropertyDefinition, PropertyType};
use crate::core::Value;
use crate::error::{GraphError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 스키마 검증 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SchemaMode {
    /// 유연 모드: 스키마 검증 안 함 (기본값)
    #[default]
    Flexible,
    /// 검증 모드: 스키마에 정의된 프로퍼티만 타입 검증
    Validate,
    /// 엄격 모드: 스키마에 정의되지 않은 프로퍼티 거부
    Strict,
}

// ============================================================================
// 스키마 변경 작업
// ============================================================================

/// 스키마 변경 작업
#[derive(Debug, Clone)]
pub enum AlterSchemaOp {
    /// 프로퍼티 추가
    AddProperty(PropertyDefinition),
    /// 프로퍼티 제거
    RemoveProperty(String),
    /// 프로퍼티 이름 변경
    RenameProperty { old_name: String, new_name: String },
    /// 프로퍼티 타입 변경
    ChangePropertyType {
        name: String,
        new_type: PropertyType,
        converter: Option<TypeConverter>,
    },
    /// 프로퍼티 필수 여부 변경
    SetPropertyRequired { name: String, required: bool },
    /// 프로퍼티 기본값 설정
    SetPropertyDefault {
        name: String,
        default: Option<Value>,
    },
}

/// 스키마 변경 빌더
#[derive(Debug, Default)]
pub struct AlterSchemaBuilder {
    operations: Vec<AlterSchemaOp>,
}

impl AlterSchemaBuilder {
    /// 새 빌더 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 프로퍼티 추가
    pub fn add_property(&mut self, prop: PropertyDefinition) -> &mut Self {
        self.operations.push(AlterSchemaOp::AddProperty(prop));
        self
    }

    /// 프로퍼티 제거
    pub fn remove_property(&mut self, name: &str) -> &mut Self {
        self.operations
            .push(AlterSchemaOp::RemoveProperty(name.to_string()));
        self
    }

    /// 프로퍼티 이름 변경
    pub fn rename_property(&mut self, old_name: &str, new_name: &str) -> &mut Self {
        self.operations.push(AlterSchemaOp::RenameProperty {
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
        });
        self
    }

    /// 프로퍼티 타입 변경
    pub fn change_property_type(
        &mut self,
        name: &str,
        new_type: PropertyType,
        converter: Option<TypeConverter>,
    ) -> &mut Self {
        self.operations.push(AlterSchemaOp::ChangePropertyType {
            name: name.to_string(),
            new_type,
            converter,
        });
        self
    }

    /// 프로퍼티 필수 여부 설정
    pub fn set_required(&mut self, name: &str, required: bool) -> &mut Self {
        self.operations.push(AlterSchemaOp::SetPropertyRequired {
            name: name.to_string(),
            required,
        });
        self
    }

    /// 프로퍼티 기본값 설정
    pub fn set_default(&mut self, name: &str, default: Option<Value>) -> &mut Self {
        self.operations.push(AlterSchemaOp::SetPropertyDefault {
            name: name.to_string(),
            default,
        });
        self
    }

    /// 작업 목록 반환
    pub fn build(self) -> Vec<AlterSchemaOp> {
        self.operations
    }
}

/// 마이그레이션 빌더
#[derive(Debug)]
pub struct MigrationBuilder {
    from_version: String,
    to_version: String,
    operations: Vec<MigrationOp>,
    description: String,
}

impl MigrationBuilder {
    /// 새 마이그레이션 빌더 생성
    pub fn new(from: &str, to: &str) -> Self {
        Self {
            from_version: from.to_string(),
            to_version: to.to_string(),
            operations: Vec::new(),
            description: String::new(),
        }
    }

    /// 노드 라벨에 프로퍼티 추가
    pub fn add_node_property(
        &mut self,
        label: &str,
        name: &str,
        prop_type: PropertyType,
    ) -> &mut Self {
        self.operations
            .push(MigrationOp::add_node_property(label, name, prop_type));
        self
    }

    /// 노드 라벨에 필수 프로퍼티 추가
    pub fn add_node_property_required(
        &mut self,
        label: &str,
        name: &str,
        prop_type: PropertyType,
        default: Value,
    ) -> &mut Self {
        self.operations
            .push(MigrationOp::add_node_property_required(
                label, name, prop_type, default,
            ));
        self
    }

    /// 노드 라벨에서 프로퍼티 제거
    pub fn remove_node_property(&mut self, label: &str, name: &str) -> &mut Self {
        self.operations
            .push(MigrationOp::remove_node_property(label, name));
        self
    }

    /// 노드 프로퍼티 이름 변경
    pub fn rename_node_property(
        &mut self,
        label: &str,
        old_name: &str,
        new_name: &str,
    ) -> &mut Self {
        self.operations
            .push(MigrationOp::rename_node_property(label, old_name, new_name));
        self
    }

    /// 노드 프로퍼티 타입 변경
    pub fn change_node_property_type(
        &mut self,
        label: &str,
        name: &str,
        new_type: PropertyType,
        converter: Option<TypeConverter>,
    ) -> &mut Self {
        self.operations.push(MigrationOp::change_node_property_type(
            label, name, new_type, converter,
        ));
        self
    }

    /// 관계 타입에 프로퍼티 추가
    pub fn add_rel_property(
        &mut self,
        rel_type: &str,
        name: &str,
        prop_type: PropertyType,
    ) -> &mut Self {
        self.operations
            .push(MigrationOp::add_rel_property(rel_type, name, prop_type));
        self
    }

    /// 관계 타입에서 프로퍼티 제거
    pub fn remove_rel_property(&mut self, rel_type: &str, name: &str) -> &mut Self {
        self.operations
            .push(MigrationOp::remove_rel_property(rel_type, name));
        self
    }

    /// 노드 라벨 추가
    pub fn add_label(&mut self, schema: NodeLabelSchema) -> &mut Self {
        self.operations.push(MigrationOp::add_label(schema));
        self
    }

    /// 노드 라벨 제거
    pub fn remove_label(&mut self, label: &str) -> &mut Self {
        self.operations.push(MigrationOp::remove_label(label));
        self
    }

    /// 관계 타입 추가
    pub fn add_relationship_type(&mut self, schema: RelationshipTypeSchema) -> &mut Self {
        self.operations
            .push(MigrationOp::add_relationship_type(schema));
        self
    }

    /// 관계 타입 제거
    pub fn remove_relationship_type(&mut self, rel_type: &str) -> &mut Self {
        self.operations
            .push(MigrationOp::remove_relationship_type(rel_type));
        self
    }

    /// 마이그레이션 설명 설정
    pub fn description(&mut self, desc: &str) -> &mut Self {
        self.description = desc.to_string();
        self
    }

    /// 마이그레이션 빌드
    pub fn build(self) -> Result<SchemaMigration> {
        let mut migration = SchemaMigration::new(&self.from_version, &self.to_version)?;
        for op in self.operations {
            migration = migration.with_op(op);
        }
        if !self.description.is_empty() {
            migration = migration.with_description(&self.description);
        }
        Ok(migration)
    }
}

// ============================================================================
// 스키마 관리자
// ============================================================================

/// 스키마 관리자
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchemaManager {
    /// 노드 라벨 스키마 (라벨 -> 스키마)
    node_schemas: HashMap<String, NodeLabelSchema>,
    /// 관계 타입 스키마 (타입 -> 스키마)
    relationship_schemas: HashMap<String, RelationshipTypeSchema>,
    /// 스키마 검증 모드
    #[serde(default)]
    mode: SchemaMode,
    /// 스키마 진화 관리자
    #[serde(default)]
    evolution_manager: SchemaEvolutionManager,
}

impl SchemaManager {
    /// 새 스키마 관리자 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 스키마 모드 설정
    pub fn set_mode(&mut self, mode: SchemaMode) {
        self.mode = mode;
    }

    /// 현재 스키마 모드 반환
    pub fn mode(&self) -> SchemaMode {
        self.mode
    }

    // ========== 노드 라벨 스키마 관리 ==========

    /// 노드 라벨 스키마 정의
    pub fn define_node_label(&mut self, schema: NodeLabelSchema) -> Result<()> {
        let label = schema.label.clone();
        if self.node_schemas.contains_key(&label) {
            return Err(GraphError::Schema(format!(
                "Node label schema '{}' already exists",
                label
            )));
        }
        self.node_schemas.insert(label, schema);
        Ok(())
    }

    /// 노드 라벨 스키마 조회
    pub fn get_node_label_schema(&self, label: &str) -> Option<&NodeLabelSchema> {
        self.node_schemas.get(label)
    }

    /// 노드 라벨 스키마 삭제
    pub fn drop_node_label_schema(&mut self, label: &str) -> Result<()> {
        if self.node_schemas.remove(label).is_none() {
            return Err(GraphError::Schema(format!(
                "Node label schema '{}' not found",
                label
            )));
        }
        Ok(())
    }

    /// 모든 노드 라벨 목록 반환
    pub fn list_node_labels(&self) -> Vec<&str> {
        self.node_schemas.keys().map(|s| s.as_str()).collect()
    }

    /// 노드 라벨 스키마 존재 여부 확인
    pub fn has_node_label_schema(&self, label: &str) -> bool {
        self.node_schemas.contains_key(label)
    }

    // ========== 관계 타입 스키마 관리 ==========

    /// 관계 타입 스키마 정의
    pub fn define_relationship_type(&mut self, schema: RelationshipTypeSchema) -> Result<()> {
        let rel_type = schema.relationship_type.clone();
        if self.relationship_schemas.contains_key(&rel_type) {
            return Err(GraphError::Schema(format!(
                "Relationship type schema '{}' already exists",
                rel_type
            )));
        }
        self.relationship_schemas.insert(rel_type, schema);
        Ok(())
    }

    /// 관계 타입 스키마 조회
    pub fn get_relationship_type_schema(&self, rel_type: &str) -> Option<&RelationshipTypeSchema> {
        self.relationship_schemas.get(rel_type)
    }

    /// 관계 타입 스키마 삭제
    pub fn drop_relationship_type_schema(&mut self, rel_type: &str) -> Result<()> {
        if self.relationship_schemas.remove(rel_type).is_none() {
            return Err(GraphError::Schema(format!(
                "Relationship type schema '{}' not found",
                rel_type
            )));
        }
        Ok(())
    }

    /// 모든 관계 타입 목록 반환
    pub fn list_relationship_types(&self) -> Vec<&str> {
        self.relationship_schemas
            .keys()
            .map(|s| s.as_str())
            .collect()
    }

    /// 관계 타입 스키마 존재 여부 확인
    pub fn has_relationship_type_schema(&self, rel_type: &str) -> bool {
        self.relationship_schemas.contains_key(rel_type)
    }

    // ========== 검증 ==========

    /// 노드 프로퍼티 검증
    ///
    /// # Arguments
    /// * `label` - 노드 라벨
    /// * `properties` - 프로퍼티 맵
    ///
    /// # Returns
    /// * `Ok(())` - 검증 성공 또는 스키마 없음/유연 모드
    /// * `Err` - 검증 실패
    pub fn validate_node(&self, label: &str, properties: &IndexMap<String, Value>) -> Result<()> {
        if self.mode == SchemaMode::Flexible {
            return Ok(());
        }

        if let Some(schema) = self.node_schemas.get(label) {
            let strict = self.mode == SchemaMode::Strict;
            schema
                .validate(properties, strict)
                .map_err(|errors| GraphError::Schema(errors.join("; ")))
        } else {
            // 스키마가 없으면 통과
            Ok(())
        }
    }

    /// 관계 프로퍼티 검증
    ///
    /// # Arguments
    /// * `rel_type` - 관계 타입
    /// * `properties` - 프로퍼티 맵
    /// * `source_label` - 소스 노드 라벨
    /// * `target_label` - 타겟 노드 라벨
    ///
    /// # Returns
    /// * `Ok(())` - 검증 성공 또는 스키마 없음/유연 모드
    /// * `Err` - 검증 실패
    pub fn validate_relationship(
        &self,
        rel_type: &str,
        properties: &IndexMap<String, Value>,
        source_label: Option<&str>,
        target_label: Option<&str>,
    ) -> Result<()> {
        if self.mode == SchemaMode::Flexible {
            return Ok(());
        }

        if let Some(schema) = self.relationship_schemas.get(rel_type) {
            let strict = self.mode == SchemaMode::Strict;
            schema
                .validate(properties, source_label, target_label, strict)
                .map_err(|errors| GraphError::Schema(errors.join("; ")))
        } else {
            Ok(())
        }
    }

    /// 노드 프로퍼티에 기본값 적용
    pub fn apply_node_defaults(&self, label: &str, properties: &mut IndexMap<String, Value>) {
        if let Some(schema) = self.node_schemas.get(label) {
            schema.apply_defaults(properties);
        }
    }

    /// 관계 프로퍼티에 기본값 적용
    pub fn apply_relationship_defaults(
        &self,
        rel_type: &str,
        properties: &mut IndexMap<String, Value>,
    ) {
        if let Some(schema) = self.relationship_schemas.get(rel_type) {
            schema.apply_defaults(properties);
        }
    }

    // ========== 직렬화 ==========

    /// JSON 문자열로 직렬화
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| GraphError::Serialization {
            message: format!("Failed to serialize schema: {}", e),
            source: None,
        })
    }

    /// JSON 문자열에서 역직렬화
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| GraphError::Serialization {
            message: format!("Failed to deserialize schema: {}", e),
            source: None,
        })
    }

    /// 스키마 통계 반환
    pub fn stats(&self) -> SchemaStats {
        SchemaStats {
            node_label_count: self.node_schemas.len(),
            relationship_type_count: self.relationship_schemas.len(),
            mode: self.mode,
        }
    }

    /// 스키마 초기화 (모든 스키마 삭제)
    pub fn clear(&mut self) {
        self.node_schemas.clear();
        self.relationship_schemas.clear();
    }

    // ========== 스키마 진화 API ==========

    /// 노드 라벨 스키마 변경
    ///
    /// 콜백 함수를 통해 스키마 변경 작업을 정의합니다.
    ///
    /// # Arguments
    /// * `label` - 변경할 노드 라벨
    /// * `f` - 변경 작업을 정의하는 콜백
    ///
    /// # Example
    /// ```rust,ignore
    /// manager.alter_node_schema("Person", |ops| {
    ///     ops.add_property(PropertyDefinition::new("email", PropertyType::String));
    ///     ops.remove_property("old_field");
    /// })?;
    /// ```
    pub fn alter_node_schema<F>(&mut self, label: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut AlterSchemaBuilder),
    {
        let schema = self.node_schemas.get(label).ok_or_else(|| {
            GraphError::Schema(format!("Node label schema '{}' not found", label))
        })?;

        let mut builder = AlterSchemaBuilder::new();
        f(&mut builder);
        let operations = builder.build();

        if operations.is_empty() {
            return Ok(());
        }

        // 스키마 복사 후 수정
        let mut new_schema = schema.clone();

        for op in operations {
            match op {
                AlterSchemaOp::AddProperty(prop) => {
                    if new_schema.properties.contains_key(&prop.name) {
                        return Err(GraphError::Schema(format!(
                            "Property '{}' already exists in schema '{}'",
                            prop.name, label
                        )));
                    }
                    new_schema.properties.insert(prop.name.clone(), prop);
                }
                AlterSchemaOp::RemoveProperty(name) => {
                    if !new_schema.properties.contains_key(&name) {
                        return Err(GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            name, label
                        )));
                    }
                    new_schema.properties.remove(&name);
                }
                AlterSchemaOp::RenameProperty { old_name, new_name } => {
                    let prop = new_schema.properties.remove(&old_name).ok_or_else(|| {
                        GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            old_name, label
                        ))
                    })?;
                    if new_schema.properties.contains_key(&new_name) {
                        return Err(GraphError::Schema(format!(
                            "Property '{}' already exists in schema '{}'",
                            new_name, label
                        )));
                    }
                    let mut renamed = prop;
                    renamed.name = new_name.clone();
                    new_schema.properties.insert(new_name, renamed);
                }
                AlterSchemaOp::ChangePropertyType { name, new_type, .. } => {
                    let prop = new_schema.properties.get_mut(&name).ok_or_else(|| {
                        GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            name, label
                        ))
                    })?;
                    prop.property_type = new_type;
                }
                AlterSchemaOp::SetPropertyRequired { name, required } => {
                    let prop = new_schema.properties.get_mut(&name).ok_or_else(|| {
                        GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            name, label
                        ))
                    })?;
                    // required가 true면 nullable은 false
                    prop.nullable = !required;
                }
                AlterSchemaOp::SetPropertyDefault { name, default } => {
                    let prop = new_schema.properties.get_mut(&name).ok_or_else(|| {
                        GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            name, label
                        ))
                    })?;
                    prop.default_value = default;
                }
            }
        }

        // 스키마 교체
        self.node_schemas.insert(label.to_string(), new_schema);
        Ok(())
    }

    /// 관계 타입 스키마 변경
    ///
    /// 콜백 함수를 통해 스키마 변경 작업을 정의합니다.
    ///
    /// # Arguments
    /// * `rel_type` - 변경할 관계 타입
    /// * `f` - 변경 작업을 정의하는 콜백
    ///
    /// # Example
    /// ```rust,ignore
    /// manager.alter_rel_schema("KNOWS", |ops| {
    ///     ops.add_property(PropertyDefinition::new("weight", PropertyType::Float));
    /// })?;
    /// ```
    pub fn alter_rel_schema<F>(&mut self, rel_type: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut AlterSchemaBuilder),
    {
        let schema = self.relationship_schemas.get(rel_type).ok_or_else(|| {
            GraphError::Schema(format!("Relationship type schema '{}' not found", rel_type))
        })?;

        let mut builder = AlterSchemaBuilder::new();
        f(&mut builder);
        let operations = builder.build();

        if operations.is_empty() {
            return Ok(());
        }

        // 스키마 복사 후 수정
        let mut new_schema = schema.clone();

        for op in operations {
            match op {
                AlterSchemaOp::AddProperty(prop) => {
                    if new_schema.properties.contains_key(&prop.name) {
                        return Err(GraphError::Schema(format!(
                            "Property '{}' already exists in schema '{}'",
                            prop.name, rel_type
                        )));
                    }
                    new_schema.properties.insert(prop.name.clone(), prop);
                }
                AlterSchemaOp::RemoveProperty(name) => {
                    if !new_schema.properties.contains_key(&name) {
                        return Err(GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            name, rel_type
                        )));
                    }
                    new_schema.properties.remove(&name);
                }
                AlterSchemaOp::RenameProperty { old_name, new_name } => {
                    let prop = new_schema.properties.remove(&old_name).ok_or_else(|| {
                        GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            old_name, rel_type
                        ))
                    })?;
                    if new_schema.properties.contains_key(&new_name) {
                        return Err(GraphError::Schema(format!(
                            "Property '{}' already exists in schema '{}'",
                            new_name, rel_type
                        )));
                    }
                    let mut renamed = prop;
                    renamed.name = new_name.clone();
                    new_schema.properties.insert(new_name, renamed);
                }
                AlterSchemaOp::ChangePropertyType { name, new_type, .. } => {
                    let prop = new_schema.properties.get_mut(&name).ok_or_else(|| {
                        GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            name, rel_type
                        ))
                    })?;
                    prop.property_type = new_type;
                }
                AlterSchemaOp::SetPropertyRequired { name, required } => {
                    let prop = new_schema.properties.get_mut(&name).ok_or_else(|| {
                        GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            name, rel_type
                        ))
                    })?;
                    // required가 true면 nullable은 false
                    prop.nullable = !required;
                }
                AlterSchemaOp::SetPropertyDefault { name, default } => {
                    let prop = new_schema.properties.get_mut(&name).ok_or_else(|| {
                        GraphError::Schema(format!(
                            "Property '{}' not found in schema '{}'",
                            name, rel_type
                        ))
                    })?;
                    prop.default_value = default;
                }
            }
        }

        // 스키마 교체
        self.relationship_schemas
            .insert(rel_type.to_string(), new_schema);
        Ok(())
    }

    /// 버전 기반 스키마 마이그레이션
    ///
    /// 콜백 함수를 통해 마이그레이션 작업을 정의하고 적용합니다.
    ///
    /// # Arguments
    /// * `from_version` - 시작 버전 (예: "1.0.0")
    /// * `to_version` - 대상 버전 (예: "1.1.0")
    /// * `f` - 마이그레이션 작업을 정의하는 콜백
    ///
    /// # Example
    /// ```rust,ignore
    /// let result = manager.migrate_schema("1.0.0", "1.1.0", |m| {
    ///     m.add_node_property("Person", "email", PropertyType::String);
    ///     m.description("Add email property to Person");
    /// })?;
    /// ```
    pub fn migrate_schema<F>(
        &mut self,
        from_version: &str,
        to_version: &str,
        f: F,
    ) -> Result<MigrationResult>
    where
        F: FnOnce(&mut MigrationBuilder),
    {
        let mut builder = MigrationBuilder::new(from_version, to_version);
        f(&mut builder);
        let migration = builder.build()?;

        // borrow 문제 해결: evolution_manager를 임시로 분리
        let mut evolution_manager = std::mem::take(&mut self.evolution_manager);
        let result = evolution_manager.apply_migration(&migration, self);
        self.evolution_manager = evolution_manager;
        result
    }

    /// 현재 스키마 버전 반환
    pub fn schema_version(&self) -> String {
        self.evolution_manager.current_version().to_string()
    }

    /// 스키마 진화 관리자 참조 반환
    pub fn evolution_manager(&self) -> &SchemaEvolutionManager {
        &self.evolution_manager
    }

    /// 스키마 진화 관리자 가변 참조 반환
    pub fn evolution_manager_mut(&mut self) -> &mut SchemaEvolutionManager {
        &mut self.evolution_manager
    }

    /// 스키마 버전 히스토리 반환
    pub fn version_history(&self) -> Vec<String> {
        self.evolution_manager
            .version_history()
            .iter()
            .map(|v| v.to_string())
            .collect()
    }

    /// 마지막 마이그레이션 롤백
    pub fn rollback_migration(&mut self) -> Result<()> {
        // borrow 문제 해결: evolution_manager를 임시로 분리
        let mut evolution_manager = std::mem::take(&mut self.evolution_manager);
        let result = evolution_manager.rollback(self);
        self.evolution_manager = evolution_manager;
        result
    }
}

/// 스키마 통계
#[derive(Debug, Clone)]
pub struct SchemaStats {
    /// 노드 라벨 스키마 수
    pub node_label_count: usize,
    /// 관계 타입 스키마 수
    pub relationship_type_count: usize,
    /// 스키마 모드
    pub mode: SchemaMode,
}
