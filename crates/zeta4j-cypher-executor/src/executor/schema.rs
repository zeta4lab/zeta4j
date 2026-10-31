// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 스키마, 인덱스, 제약조건 관리 모듈
//!
//! 이 모듈은 다음 명령들을 처리합니다:
//! - CREATE/DROP/SHOW INDEX
//! - CREATE/DROP/SHOW CONSTRAINT
//! - CREATE/DROP/SHOW SCHEMA

use indexmap::IndexMap;

use super::QueryExecutor;
use zeta4j_cypher_core::ast::{ConstraintCommand, IndexCommand, SchemaCommand};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};
use zeta4j_graph::transaction::TransactionId;

impl QueryExecutor {
    // ============================================================
    // 인덱스 관리 명령 실행
    // ============================================================

    /// 인덱스 관리 명령 실행 (CREATE INDEX, DROP INDEX, SHOW INDEXES)
    pub(super) async fn execute_index_command(
        &self,
        cmd: &IndexCommand,
        _tx_id: TransactionId,
    ) -> Result<Vec<Vec<Value>>> {
        match cmd {
            IndexCommand::Create {
                name,
                label,
                properties,
            } => {
                // 인덱스 이름 생성 (제공되지 않으면 자동 생성)
                let index_name = name
                    .clone()
                    .unwrap_or_else(|| format!("idx_{}_{}", label, properties.join("_")));

                // GraphEngine에 인덱스 생성 요청
                self.engine
                    .create_property_index(&index_name, label, properties)
                    .await?;

                // 인덱스 캐시 무효화
                for prop in properties {
                    self.invalidate_index_cache(label, prop);
                }
                // 쿼리 플랜 캐시 무효화 (해당 라벨 사용 쿼리)
                self.invalidate_query_cache_by_label(label);

                Ok(vec![])
            }
            IndexCommand::Drop {
                name,
                label,
                properties,
            } => {
                if let Some(idx_name) = name {
                    // 이름으로 삭제
                    self.engine.drop_property_index(idx_name)?;
                    // 이름에서 라벨 추출이 어려우므로 전체 플랜 캐시 무효화
                    self.clear_query_cache();
                } else if let Some(lbl) = label {
                    // 라벨+프로퍼티로 삭제
                    let idx_name = format!("idx_{}_{}", lbl, properties.join("_"));
                    self.engine.drop_property_index(&idx_name)?;

                    // 인덱스 캐시 무효화
                    for prop in properties {
                        self.invalidate_index_cache(lbl, prop);
                    }
                    // 쿼리 플랜 캐시 무효화 (해당 라벨 사용 쿼리)
                    self.invalidate_query_cache_by_label(lbl);
                }
                Ok(vec![])
            }
            IndexCommand::Show => {
                // 인덱스 목록 조회
                let indexes = self.engine.list_property_indexes();
                let rows: Vec<Vec<Value>> = indexes
                    .into_iter()
                    .map(|(name, label, props)| {
                        vec![
                            Value::String(name),
                            Value::String(label),
                            Value::String(props.join(", ")),
                        ]
                    })
                    .collect();
                Ok(rows)
            }
            IndexCommand::CreateSpatial { label, property } => {
                // 공간 인덱스 생성
                self.engine.create_spatial_index(label, property).await?;
                // 쿼리 플랜 캐시 무효화 (해당 라벨 사용 쿼리)
                self.invalidate_query_cache_by_label(label);
                Ok(vec![])
            }
            IndexCommand::DropSpatial { label, property } => {
                // 공간 인덱스 삭제
                self.engine.drop_spatial_index(label, property)?;
                // 쿼리 플랜 캐시 무효화 (해당 라벨 사용 쿼리)
                self.invalidate_query_cache_by_label(label);
                Ok(vec![])
            }
            IndexCommand::ShowSpatial => {
                // 공간 인덱스 목록 조회
                let indexes = self.engine.list_spatial_indexes();
                let rows: Vec<Vec<Value>> = indexes
                    .into_iter()
                    .map(|(label, prop)| {
                        vec![
                            Value::String("SPATIAL".to_string()),
                            Value::String(label),
                            Value::String(prop),
                        ]
                    })
                    .collect();
                Ok(rows)
            }
            IndexCommand::CreateFulltext {
                name,
                label,
                properties,
                analyzer,
            } => {
                self.engine
                    .create_fulltext_index(name, label, properties, analyzer)
                    .await?;
                self.invalidate_query_cache_by_label(label);
                Ok(Vec::new())
            }
            IndexCommand::DropFulltext { name } => {
                self.engine.drop_fulltext_index(name)?;
                self.clear_query_cache();
                Ok(Vec::new())
            }
            IndexCommand::ShowFulltext => self
                .engine
                .list_fulltext_indexes()?
                .into_iter()
                .map(|definition| {
                    let zeta4j_graph::engine::GraphIndexKind::Fulltext { analyzer } =
                        definition.kind
                    else {
                        return Err(GraphError::System {
                            message: "non-fulltext definition escaped fulltext filter".to_string(),
                        });
                    };
                    Ok(vec![
                        Value::String(definition.name),
                        Value::String(definition.label),
                        Value::String(definition.properties.join(", ")),
                        Value::String(analyzer),
                    ])
                })
                .collect(),
            IndexCommand::CreateVector {
                name,
                label,
                property,
                dimensions,
                distance,
            } => {
                self.engine
                    .create_vector_index(name, label, property, *dimensions, distance)
                    .await?;
                self.invalidate_query_cache_by_label(label);
                Ok(Vec::new())
            }
            IndexCommand::DropVector { name } => {
                self.engine.drop_vector_index(name)?;
                self.clear_query_cache();
                Ok(Vec::new())
            }
            IndexCommand::ShowVector => self
                .engine
                .list_vector_indexes()?
                .into_iter()
                .map(|definition| {
                    let zeta4j_graph::engine::GraphIndexKind::Vector {
                        dimensions,
                        distance,
                    } = definition.kind
                    else {
                        return Err(GraphError::System {
                            message: "non-vector definition escaped vector filter".to_string(),
                        });
                    };
                    Ok(vec![
                        Value::String(definition.name),
                        Value::String(definition.label),
                        Value::String(definition.properties.join(", ")),
                        Value::Integer(i64::from(dimensions)),
                        Value::String(distance),
                    ])
                })
                .collect(),
        }
    }

    // ============================================================
    // 스키마 관리 명령 실행
    // ============================================================

    /// 스키마 관리 명령 실행
    pub(super) fn execute_schema_command(&self, cmd: &SchemaCommand) -> Result<Vec<Vec<Value>>> {
        use zeta4j_graph::schema::{
            ElementSchema, NodeLabelSchema, PropertyDefinition, PropertyType,
            RelationshipTypeSchema,
        };

        match cmd {
            SchemaCommand::CreateNodeLabel { label, properties } => {
                let mut schema = NodeLabelSchema::new(label);

                for prop_def in properties {
                    let prop_type =
                        PropertyType::parse(&prop_def.property_type).ok_or_else(|| {
                            GraphError::Parse {
                                message: format!(
                                    "Unknown property type: {}",
                                    prop_def.property_type
                                ),
                            }
                        })?;

                    let mut definition = PropertyDefinition::new(&prop_def.name, prop_type);
                    if !prop_def.nullable {
                        definition = definition.not_null();
                    }
                    if let Some(default) = &prop_def.default_value {
                        definition = definition.with_default(default.clone());
                    }
                    schema.add_property(definition);
                }

                self.engine.define_node_label_schema(schema)?;
                Ok(vec![])
            }
            SchemaCommand::CreateRelationshipType {
                rel_type,
                properties,
                from_labels,
                to_labels,
            } => {
                let mut schema = RelationshipTypeSchema::new(rel_type);

                for prop_def in properties {
                    let prop_type =
                        PropertyType::parse(&prop_def.property_type).ok_or_else(|| {
                            GraphError::Parse {
                                message: format!(
                                    "Unknown property type: {}",
                                    prop_def.property_type
                                ),
                            }
                        })?;

                    let mut definition = PropertyDefinition::new(&prop_def.name, prop_type);
                    if !prop_def.nullable {
                        definition = definition.not_null();
                    }
                    if let Some(default) = &prop_def.default_value {
                        definition = definition.with_default(default.clone());
                    }
                    schema.add_property(definition);
                }

                if let Some(from) = from_labels {
                    schema = schema.from_labels(from.clone());
                }
                if let Some(to) = to_labels {
                    schema = schema.to_labels(to.clone());
                }

                self.engine.define_relationship_type_schema(schema)?;
                Ok(vec![])
            }
            SchemaCommand::DropNodeLabel { label } => {
                self.engine.drop_node_label_schema(label)?;
                Ok(vec![])
            }
            SchemaCommand::DropRelationshipType { rel_type } => {
                self.engine.drop_relationship_type_schema(rel_type)?;
                Ok(vec![])
            }
            SchemaCommand::ShowNodeLabels => {
                let labels = self.engine.list_node_label_schemas();
                let rows: Vec<Vec<Value>> = labels
                    .into_iter()
                    .map(|label| vec![Value::String(label)])
                    .collect();
                Ok(rows)
            }
            SchemaCommand::ShowRelationshipTypes => {
                let types = self.engine.list_relationship_type_schemas();
                let rows: Vec<Vec<Value>> =
                    types.into_iter().map(|t| vec![Value::String(t)]).collect();
                Ok(rows)
            }
            SchemaCommand::ShowSchemaFor { is_node: _, name } => {
                // 노드 라벨 스키마 확인
                if let Some(schema) = self.engine.get_node_label_schema(name) {
                    let mut rows = Vec::new();
                    rows.push(vec![
                        Value::String("Node Label".to_string()),
                        Value::String(schema.label.clone()),
                    ]);
                    for (prop_name, prop_def) in &schema.properties {
                        rows.push(vec![
                            Value::String(format!("  Property: {}", prop_name)),
                            Value::String(format!(
                                "{} {}",
                                prop_def.property_type,
                                if prop_def.nullable { "" } else { "NOT NULL" }
                            )),
                        ]);
                    }
                    return Ok(rows);
                }

                // 관계 타입 스키마 확인
                if let Some(schema) = self.engine.get_relationship_type_schema(name) {
                    let mut rows = Vec::new();
                    rows.push(vec![
                        Value::String("Relationship Type".to_string()),
                        Value::String(schema.relationship_type.clone()),
                    ]);
                    for (prop_name, prop_def) in &schema.properties {
                        rows.push(vec![
                            Value::String(format!("  Property: {}", prop_name)),
                            Value::String(format!(
                                "{} {}",
                                prop_def.property_type,
                                if prop_def.nullable { "" } else { "NOT NULL" }
                            )),
                        ]);
                    }
                    return Ok(rows);
                }

                Err(GraphError::Schema(format!(
                    "Schema not found for: {}",
                    name
                )))
            }
        }
    }

    // ============================================================
    // 제약조건 관리 명령 실행
    // ============================================================

    /// 제약조건 명령 실행
    pub(super) fn execute_constraint_command(
        &self,
        cmd: &ConstraintCommand,
    ) -> Result<Vec<Vec<Value>>> {
        use zeta4j_graph::schema::{ConstraintDefinition, ConstraintTarget, ConstraintType};

        match cmd {
            ConstraintCommand::CreateUnique {
                name,
                label,
                properties,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Unique,
                    ConstraintTarget::NodeLabel(label.clone()),
                    properties.clone(),
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateNotNull {
                name,
                label,
                properties,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::NotNull,
                    ConstraintTarget::NodeLabel(label.clone()),
                    properties.clone(),
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateNodeKey {
                name,
                label,
                properties,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::NodeKey,
                    ConstraintTarget::NodeLabel(label.clone()),
                    properties.clone(),
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::Drop { name } => {
                self.engine.remove_constraint(name)?;
                Ok(vec![vec![Value::String(format!(
                    "Constraint '{}' dropped",
                    name
                ))]])
            }
            ConstraintCommand::Show => {
                let constraints = self.engine.list_constraints();
                let rows: Vec<Vec<Value>> = constraints
                    .into_iter()
                    .map(|c| {
                        vec![
                            Value::String(c.name.clone()),
                            Value::String(format!("{:?}", c.constraint_type)),
                            Value::String(match &c.target {
                                ConstraintTarget::NodeLabel(l) => format!(":{}", l),
                                ConstraintTarget::RelationshipType(t) => format!(":{}", t),
                            }),
                            Value::String(c.properties.join(", ")),
                        ]
                    })
                    .collect();
                Ok(rows)
            }
            ConstraintCommand::ShowFor { label } => {
                let constraints = self.engine.get_constraints_for_label(label);
                let rows: Vec<Vec<Value>> = constraints
                    .into_iter()
                    .map(|c| {
                        vec![
                            Value::String(c.name.clone()),
                            Value::String(format!("{:?}", c.constraint_type)),
                            Value::String(c.properties.join(", ")),
                        ]
                    })
                    .collect();
                Ok(rows)
            }

            // ============================================================
            // 프로퍼티 레벨 제약 (신규)
            // ============================================================
            ConstraintCommand::CreateRange {
                name,
                label,
                property,
                min,
                max,
                inclusive,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Range {
                        min: *min,
                        max: *max,
                        inclusive: *inclusive,
                    },
                    ConstraintTarget::NodeLabel(label.clone()),
                    vec![property.clone()],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Range constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateLength {
                name,
                label,
                property,
                min_len,
                max_len,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Length {
                        min: *min_len,
                        max: *max_len,
                    },
                    ConstraintTarget::NodeLabel(label.clone()),
                    vec![property.clone()],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Length constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreatePattern {
                name,
                label,
                property,
                regex,
                message: _,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Pattern {
                        regex: regex.clone(),
                    },
                    ConstraintTarget::NodeLabel(label.clone()),
                    vec![property.clone()],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Pattern constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateEnum {
                name,
                label,
                property,
                values,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Enum {
                        values: values.clone(),
                    },
                    ConstraintTarget::NodeLabel(label.clone()),
                    vec![property.clone()],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Enum constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateImmutable {
                name,
                label,
                property,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Immutable,
                    ConstraintTarget::NodeLabel(label.clone()),
                    vec![property.clone()],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Immutable constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateDefault {
                name,
                label,
                property,
                value,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Default {
                        value: value.clone(),
                    },
                    ConstraintTarget::NodeLabel(label.clone()),
                    vec![property.clone()],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Default constraint '{}' created",
                    name
                ))]])
            }

            // ============================================================
            // 클래스 레벨 제약 (신규)
            // ============================================================
            ConstraintCommand::CreateCheck {
                name,
                label,
                expression,
                message: _,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Check {
                        expression: expression.clone(),
                    },
                    ConstraintTarget::NodeLabel(label.clone()),
                    vec![],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Check constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateRequireAtLeastOne {
                name,
                label,
                properties,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::RequireAtLeastOne,
                    ConstraintTarget::NodeLabel(label.clone()),
                    properties.clone(),
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "RequireAtLeastOne constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateMutuallyExclusive {
                name,
                label,
                properties,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::MutuallyExclusive,
                    ConstraintTarget::NodeLabel(label.clone()),
                    properties.clone(),
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "MutuallyExclusive constraint '{}' created",
                    name
                ))]])
            }

            // ============================================================
            // 관계 레벨 제약 (신규)
            // ============================================================
            ConstraintCommand::CreateCardinality {
                name,
                rel_type,
                min,
                max,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Cardinality {
                        min: *min,
                        max: *max,
                    },
                    ConstraintTarget::RelationshipType(rel_type.clone()),
                    vec![],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Cardinality constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateCascade {
                name,
                rel_type,
                action,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::Cascade { action: *action },
                    ConstraintTarget::RelationshipType(rel_type.clone()),
                    vec![],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "Cascade constraint '{}' created",
                    name
                ))]])
            }
            ConstraintCommand::CreateReferentialIntegrity {
                name,
                rel_type,
                on_delete,
                on_update,
            } => {
                let constraint = ConstraintDefinition::new(
                    name.clone(),
                    ConstraintType::ReferentialIntegrity {
                        on_delete: *on_delete,
                        on_update: *on_update,
                    },
                    ConstraintTarget::RelationshipType(rel_type.clone()),
                    vec![],
                );
                self.engine.add_constraint(constraint)?;
                Ok(vec![vec![Value::String(format!(
                    "ReferentialIntegrity constraint '{}' created",
                    name
                ))]])
            }
        }
    }

    // ============================================================
    // Default 제약조건 적용
    // ============================================================

    /// 누락된 프로퍼티에 Default 값 적용
    pub(super) fn apply_default_constraints(
        &self,
        labels: &[String],
        properties: &mut IndexMap<String, Value>,
    ) -> Result<()> {
        for label in labels {
            let constraints = self.engine.get_constraints_for_label(label);
            for constraint in constraints {
                if let zeta4j_graph::schema::ConstraintType::Default { ref value } =
                    constraint.constraint_type
                {
                    for prop in &constraint.properties {
                        // 프로퍼티가 없거나 Null인 경우에만 기본값 적용
                        if !properties.contains_key(prop)
                            || properties.get(prop) == Some(&Value::Null)
                        {
                            properties.insert(prop.clone(), value.clone());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // ============================================================
    // 제약조건 검증
    // ============================================================

    /// 노드 생성 시 제약조건 검증 및 UNIQUE 인덱스 등록
    ///
    /// v1.12.0: async로 변경 (CHECK 제약조건의 evaluate_expression 비동기화)
    pub(super) async fn validate_node_constraints(
        &self,
        node_id: u64,
        labels: &[String],
        properties: &IndexMap<String, Value>,
    ) -> Result<()> {
        // 각 라벨에 대해 제약조건 검증
        for label in labels {
            let constraints = self.engine.get_constraints_for_label(label);
            for constraint in constraints {
                match constraint.constraint_type {
                    zeta4j_graph::schema::ConstraintType::NotNull => {
                        // NOT NULL 검증
                        for prop in &constraint.properties {
                            if !properties.contains_key(prop)
                                || properties.get(prop) == Some(&Value::Null)
                            {
                                return Err(GraphError::Constraint {
                                    constraint_name: "validation".to_string(),
                                    message: format!(
                                        "NOT NULL constraint '{}' violated: property '{}' is required on :{}",
                                        constraint.name, prop, label
                                    ),
                                });
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Unique => {
                        // UNIQUE 검증 - ConstraintManager의 인덱스 확인
                        self.engine.constraint_manager.read().validate_unique(
                            &constraint,
                            properties,
                            None,
                        )?;
                    }
                    zeta4j_graph::schema::ConstraintType::NodeKey => {
                        // NODE KEY 검증 (NOT NULL + UNIQUE)
                        for prop in &constraint.properties {
                            if !properties.contains_key(prop)
                                || properties.get(prop) == Some(&Value::Null)
                            {
                                return Err(GraphError::Constraint {
                                    constraint_name: "validation".to_string(),
                                    message: format!(
                                        "NODE KEY constraint '{}' violated: property '{}' is required on :{}",
                                        constraint.name, prop, label
                                    ),
                                });
                            }
                        }
                        self.engine.constraint_manager.read().validate_unique(
                            &constraint,
                            properties,
                            None,
                        )?;
                    }
                    zeta4j_graph::schema::ConstraintType::RelationshipKey => {
                        // 관계 제약조건은 노드에 적용되지 않음
                    }
                    // 프로퍼티 레벨 제약
                    zeta4j_graph::schema::ConstraintType::Range {
                        min,
                        max,
                        inclusive,
                    } => {
                        for prop in &constraint.properties {
                            if let Some(value) = properties.get(prop) {
                                let num = match value {
                                    Value::Integer(i) => Some(*i as f64),
                                    Value::Float(f) => Some(*f),
                                    _ => None,
                                };
                                if let Some(n) = num {
                                    let min_ok =
                                        min.is_none_or(|m| if inclusive { n >= m } else { n > m });
                                    let max_ok =
                                        max.is_none_or(|m| if inclusive { n <= m } else { n < m });
                                    if !min_ok || !max_ok {
                                        return Err(GraphError::Constraint {
                                            constraint_name: "validation".to_string(),
                                            message: format!(
                                                "RANGE constraint '{}' violated: property '{}' value {} is out of range on :{}",
                                                constraint.name, prop, n, label
                                            ),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Length { min, max } => {
                        for prop in &constraint.properties {
                            if let Some(Value::String(s)) = properties.get(prop) {
                                let len = s.len();
                                if let Some(min_len) = min
                                    && len < min_len
                                {
                                    return Err(GraphError::Constraint {
                                        constraint_name: "validation".to_string(),
                                        message: format!(
                                            "LENGTH constraint '{}' violated: property '{}' length {} is less than minimum {} on :{}",
                                            constraint.name, prop, len, min_len, label
                                        ),
                                    });
                                }
                                if let Some(max_len) = max
                                    && len > max_len
                                {
                                    return Err(GraphError::Constraint {
                                        constraint_name: "validation".to_string(),
                                        message: format!(
                                            "LENGTH constraint '{}' violated: property '{}' length {} exceeds maximum {} on :{}",
                                            constraint.name, prop, len, max_len, label
                                        ),
                                    });
                                }
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Pattern { ref regex } => {
                        for prop in &constraint.properties {
                            if let Some(Value::String(s)) = properties.get(prop)
                                && let Ok(re) = regex::Regex::new(regex)
                                && !re.is_match(s)
                            {
                                return Err(GraphError::Constraint {
                                    constraint_name: "validation".to_string(),
                                    message: format!(
                                        "PATTERN constraint '{}' violated: property '{}' does not match pattern '{}' on :{}",
                                        constraint.name, prop, regex, label
                                    ),
                                });
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Enum { values } => {
                        for prop in &constraint.properties {
                            if let Some(Value::String(s)) = properties.get(prop)
                                && !values.contains(s)
                            {
                                return Err(GraphError::Constraint {
                                    constraint_name: "validation".to_string(),
                                    message: format!(
                                        "ENUM constraint '{}' violated: property '{}' value '{}' is not in allowed values {:?} on :{}",
                                        constraint.name, prop, s, values, label
                                    ),
                                });
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Immutable => {
                        // CREATE 시에는 Immutable 검증 불필요 (UPDATE 시에만 적용)
                    }
                    zeta4j_graph::schema::ConstraintType::Default { .. } => {
                        // Default 값은 별도 처리 (프로퍼티 설정 시 적용)
                    }
                    // 클래스 레벨 제약
                    zeta4j_graph::schema::ConstraintType::Check { ref expression } => {
                        use zeta4j_cypher_core::parser::parse_expression;

                        // 표현식 파싱
                        match parse_expression(expression) {
                            Ok((_, expr)) => {
                                // 컨텍스트 생성: 프로퍼티를 변수로 바인딩
                                let mut context = super::ExecutionContext::new(0);
                                for (key, value) in properties {
                                    context.variables.insert(key.clone(), value.clone());
                                }

                                // 표현식 평가 (v1.12.0: async)
                                match self.evaluate_expression(&expr, &context).await {
                                    Ok(Value::Boolean(true)) => { /* 검증 통과 */ }
                                    Ok(Value::Boolean(false)) => {
                                        return Err(GraphError::Constraint {
                                            constraint_name: "validation".to_string(),
                                            message: format!(
                                                "CHECK constraint '{}' violated: expression '{}' evaluated to false on :{}",
                                                constraint.name, expression, label
                                            ),
                                        });
                                    }
                                    Ok(other) => {
                                        return Err(GraphError::Constraint {
                                            constraint_name: "validation".to_string(),
                                            message: format!(
                                                "CHECK constraint '{}' violated: expression '{}' must return boolean, got {:?} on :{}",
                                                constraint.name, expression, other, label
                                            ),
                                        });
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "CHECK constraint '{}' evaluation failed: {}",
                                            constraint.name,
                                            e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "CHECK constraint '{}' parse failed: {:?}",
                                    constraint.name,
                                    e
                                );
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::RequireAtLeastOne => {
                        let has_any = constraint
                            .properties
                            .iter()
                            .any(|p| properties.get(p).is_some_and(|v| !matches!(v, Value::Null)));
                        if !has_any {
                            return Err(GraphError::Constraint {
                                constraint_name: "validation".to_string(),
                                message: format!(
                                    "REQUIRE_AT_LEAST_ONE constraint '{}' violated: at least one of {:?} must be present on :{}",
                                    constraint.name, constraint.properties, label
                                ),
                            });
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::MutuallyExclusive => {
                        let count = constraint
                            .properties
                            .iter()
                            .filter(|p| {
                                properties
                                    .get(*p)
                                    .is_some_and(|v| !matches!(v, Value::Null))
                            })
                            .count();
                        if count > 1 {
                            return Err(GraphError::Constraint {
                                constraint_name: "validation".to_string(),
                                message: format!(
                                    "MUTUALLY_EXCLUSIVE constraint '{}' violated: only one of {:?} can be present on :{}",
                                    constraint.name, constraint.properties, label
                                ),
                            });
                        }
                    }
                    // 관계 레벨 제약 (노드에 적용 안됨)
                    zeta4j_graph::schema::ConstraintType::Cardinality { .. }
                    | zeta4j_graph::schema::ConstraintType::Cascade { .. }
                    | zeta4j_graph::schema::ConstraintType::ReferentialIntegrity { .. } => {
                        // 관계 제약조건은 노드에 적용되지 않음
                    }
                }
            }
        }

        // 검증 통과 후 UNIQUE 인덱스에 등록
        for label in labels {
            self.engine
                .constraint_manager
                .read()
                .register_unique_value(label, properties, node_id)?
        }

        Ok(())
    }

    /// 노드 업데이트 시 제약조건 검증
    ///
    /// v1.12.0: async로 변경 (CHECK 제약조건의 evaluate_expression 비동기화)
    #[expect(dead_code)]
    pub(super) async fn validate_node_update_constraints(
        &self,
        node_id: u64,
        labels: &[String],
        old_properties: &IndexMap<String, Value>,
        new_properties: &IndexMap<String, Value>,
    ) -> Result<()> {
        // 각 라벨에 대해 제약조건 검증
        for label in labels {
            let constraints = self.engine.get_constraints_for_label(label);
            for constraint in constraints {
                match constraint.constraint_type {
                    zeta4j_graph::schema::ConstraintType::NotNull => {
                        // NOT NULL 검증
                        for prop in &constraint.properties {
                            if !new_properties.contains_key(prop)
                                || new_properties.get(prop) == Some(&Value::Null)
                            {
                                return Err(GraphError::Constraint {
                                    constraint_name: "validation".to_string(),
                                    message: format!(
                                        "NOT NULL constraint '{}' violated: property '{}' is required on :{}",
                                        constraint.name, prop, label
                                    ),
                                });
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Unique => {
                        // UNIQUE 검증
                        self.engine.constraint_manager.read().validate_unique(
                            &constraint,
                            new_properties,
                            Some(node_id),
                        )?;
                    }
                    zeta4j_graph::schema::ConstraintType::NodeKey => {
                        // NODE KEY 검증 (NOT NULL + UNIQUE)
                        for prop in &constraint.properties {
                            if !new_properties.contains_key(prop)
                                || new_properties.get(prop) == Some(&Value::Null)
                            {
                                return Err(GraphError::Constraint {
                                    constraint_name: "validation".to_string(),
                                    message: format!(
                                        "NODE KEY constraint '{}' violated: property '{}' is required on :{}",
                                        constraint.name, prop, label
                                    ),
                                });
                            }
                        }
                        self.engine.constraint_manager.read().validate_unique(
                            &constraint,
                            new_properties,
                            Some(node_id),
                        )?;
                    }
                    zeta4j_graph::schema::ConstraintType::RelationshipKey => {
                        // 관계 제약조건은 노드에 적용되지 않음
                    }
                    // 프로퍼티 레벨 제약
                    zeta4j_graph::schema::ConstraintType::Range {
                        min,
                        max,
                        inclusive,
                    } => {
                        for prop in &constraint.properties {
                            if let Some(value) = new_properties.get(prop) {
                                let num = match value {
                                    Value::Integer(i) => Some(*i as f64),
                                    Value::Float(f) => Some(*f),
                                    _ => None,
                                };
                                if let Some(n) = num {
                                    let min_ok =
                                        min.is_none_or(|m| if inclusive { n >= m } else { n > m });
                                    let max_ok =
                                        max.is_none_or(|m| if inclusive { n <= m } else { n < m });
                                    if !min_ok || !max_ok {
                                        return Err(GraphError::Constraint {
                                            constraint_name: "validation".to_string(),
                                            message: format!(
                                                "RANGE constraint '{}' violated: property '{}' value {} is out of range on :{}",
                                                constraint.name, prop, n, label
                                            ),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Length { min, max } => {
                        for prop in &constraint.properties {
                            if let Some(Value::String(s)) = new_properties.get(prop) {
                                let len = s.len();
                                if let Some(min_len) = min
                                    && len < min_len
                                {
                                    return Err(GraphError::Constraint {
                                        constraint_name: "validation".to_string(),
                                        message: format!(
                                            "LENGTH constraint '{}' violated: property '{}' length {} is less than minimum {} on :{}",
                                            constraint.name, prop, len, min_len, label
                                        ),
                                    });
                                }
                                if let Some(max_len) = max
                                    && len > max_len
                                {
                                    return Err(GraphError::Constraint {
                                        constraint_name: "validation".to_string(),
                                        message: format!(
                                            "LENGTH constraint '{}' violated: property '{}' length {} exceeds maximum {} on :{}",
                                            constraint.name, prop, len, max_len, label
                                        ),
                                    });
                                }
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Pattern { ref regex } => {
                        for prop in &constraint.properties {
                            if let Some(Value::String(s)) = new_properties.get(prop)
                                && let Ok(re) = regex::Regex::new(regex)
                                && !re.is_match(s)
                            {
                                return Err(GraphError::Constraint {
                                    constraint_name: "validation".to_string(),
                                    message: format!(
                                        "PATTERN constraint '{}' violated: property '{}' does not match pattern '{}' on :{}",
                                        constraint.name, prop, regex, label
                                    ),
                                });
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Enum { values } => {
                        for prop in &constraint.properties {
                            if let Some(Value::String(s)) = new_properties.get(prop)
                                && !values.contains(s)
                            {
                                return Err(GraphError::Constraint {
                                    constraint_name: "validation".to_string(),
                                    message: format!(
                                        "ENUM constraint '{}' violated: property '{}' value '{}' is not in allowed values {:?} on :{}",
                                        constraint.name, prop, s, values, label
                                    ),
                                });
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Immutable => {
                        // UPDATE 시 Immutable 프로퍼티 변경 검사
                        for prop in &constraint.properties {
                            let old_val = old_properties.get(prop);
                            let new_val = new_properties.get(prop);
                            if old_val != new_val {
                                return Err(GraphError::Constraint {
                                    constraint_name: "validation".to_string(),
                                    message: format!(
                                        "IMMUTABLE constraint '{}' violated: property '{}' cannot be modified on :{}",
                                        constraint.name, prop, label
                                    ),
                                });
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::Default { .. } => {
                        // Default 값은 별도 처리 (프로퍼티 설정 시 적용)
                    }
                    // 클래스 레벨 제약
                    zeta4j_graph::schema::ConstraintType::Check { ref expression } => {
                        use zeta4j_cypher_core::parser::parse_expression;

                        // 표현식 파싱
                        match parse_expression(expression) {
                            Ok((_, expr)) => {
                                // 컨텍스트 생성: 프로퍼티를 변수로 바인딩
                                let mut context = super::ExecutionContext::new(0);
                                for (key, value) in new_properties.iter() {
                                    context.variables.insert(key.clone(), value.clone());
                                }

                                // 표현식 평가 (v1.12.0: async)
                                match self.evaluate_expression(&expr, &context).await {
                                    Ok(Value::Boolean(true)) => { /* 검증 통과 */ }
                                    Ok(Value::Boolean(false)) => {
                                        return Err(GraphError::Constraint {
                                            constraint_name: "validation".to_string(),
                                            message: format!(
                                                "CHECK constraint '{}' violated: expression '{}' evaluated to false on :{}",
                                                constraint.name, expression, label
                                            ),
                                        });
                                    }
                                    Ok(other) => {
                                        return Err(GraphError::Constraint {
                                            constraint_name: "validation".to_string(),
                                            message: format!(
                                                "CHECK constraint '{}' violated: expression '{}' must return boolean, got {:?} on :{}",
                                                constraint.name, expression, other, label
                                            ),
                                        });
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "CHECK constraint '{}' evaluation failed: {}",
                                            constraint.name,
                                            e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "CHECK constraint '{}' parse failed: {:?}",
                                    constraint.name,
                                    e
                                );
                            }
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::RequireAtLeastOne => {
                        let has_any = constraint.properties.iter().any(|p| {
                            new_properties
                                .get(p)
                                .is_some_and(|v| !matches!(v, Value::Null))
                        });
                        if !has_any {
                            return Err(GraphError::Constraint {
                                constraint_name: "validation".to_string(),
                                message: format!(
                                    "REQUIRE_AT_LEAST_ONE constraint '{}' violated: at least one of {:?} must be present on :{}",
                                    constraint.name, constraint.properties, label
                                ),
                            });
                        }
                    }
                    zeta4j_graph::schema::ConstraintType::MutuallyExclusive => {
                        let count = constraint
                            .properties
                            .iter()
                            .filter(|p| {
                                new_properties
                                    .get(*p)
                                    .is_some_and(|v| !matches!(v, Value::Null))
                            })
                            .count();
                        if count > 1 {
                            return Err(GraphError::Constraint {
                                constraint_name: "validation".to_string(),
                                message: format!(
                                    "MUTUALLY_EXCLUSIVE constraint '{}' violated: only one of {:?} can be present on :{}",
                                    constraint.name, constraint.properties, label
                                ),
                            });
                        }
                    }
                    // 관계 레벨 제약 (노드에 적용 안됨)
                    zeta4j_graph::schema::ConstraintType::Cardinality { .. }
                    | zeta4j_graph::schema::ConstraintType::Cascade { .. }
                    | zeta4j_graph::schema::ConstraintType::ReferentialIntegrity { .. } => {
                        // 관계 제약조건은 노드에 적용되지 않음
                    }
                }
            }
        }
        Ok(())
    }
}
