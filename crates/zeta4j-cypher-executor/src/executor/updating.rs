// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! UPDATE 절 실행 모듈
//!
//! CREATE, DELETE, SET, MERGE 쓰기 작업 처리
//!
//! # v1.12.0 비동기 API 통일
//!
//! 모든 업데이트 메서드가 async로 변경되었습니다.
//! 표현식 평가가 비동기로 처리됩니다.

use super::{ExecutionContext, QueryExecutor};
use indexmap::IndexMap;
use zeta4j_common::{ZetaError, codes};
use zeta4j_cypher_core::ast::*;
use zeta4j_graph::core::{Node, PropertyContainer, Value};
use zeta4j_graph::{GraphError, Result};

impl QueryExecutor {
    /// PropertyPattern의 Expression들을 평가하여 IndexMap<String, Value>로 변환
    ///
    /// v1.12.0: async로 변경
    async fn evaluate_property_pattern(
        &self,
        props: &PropertyPattern,
        context: &ExecutionContext,
    ) -> Result<IndexMap<String, Value>> {
        let mut result = IndexMap::new();
        for (key, expr) in props.iter() {
            let value = self.evaluate_expression(expr, context).await?;
            result.insert(key.clone(), value);
        }
        Ok(result)
    }

    /// PropertyPattern의 프로퍼티를 노드에 설정
    ///
    /// v1.12.0: async로 변경
    #[expect(dead_code)] // MERGE 등에서 사용 예정
    async fn apply_property_pattern_to_node(
        &self,
        props: &PropertyPattern,
        node: &mut Node,
        context: &ExecutionContext,
    ) -> Result<()> {
        for (key, expr) in props.iter() {
            let value = self.evaluate_expression(expr, context).await?;
            node.set_property(key, value);
        }
        Ok(())
    }
    /// UPDATE 절 실행 (CREATE, DELETE, SET, MERGE)
    ///
    /// v1.12.0: async로 변경
    #[async_recursion::async_recursion]
    pub(super) async fn execute_updating(
        &self,
        clause: &UpdatingClause,
        context: &mut ExecutionContext,
    ) -> Result<()> {
        match clause {
            UpdatingClause::Create(pattern) => {
                // 경로 패턴 지원: (a)-[:REL]->(b) 형태 처리
                if !pattern.elements.is_empty() {
                    self.execute_create_path(pattern, context).await?;
                } else {
                    // 하위 호환성: 단순 노드 패턴 (pattern.parts)
                    for part in &pattern.parts {
                        let labels = part.labels.clone();
                        let label_count = labels.len() as u64;

                        // 속성 먼저 평가 (노드 생성 전에 검증을 위해)
                        let evaluated_props = if let Some(props) = &part.properties {
                            self.evaluate_property_pattern(props, context).await?
                        } else {
                            IndexMap::new()
                        };

                        // 노드 생성
                        let node_id = self
                            .engine
                            .create_node_internal(context.tx_id, labels.clone())
                            .await?;

                        // 노드 생성 통계
                        context.mutation_stats.inc_nodes_created(1);
                        context.mutation_stats.inc_labels_added(label_count);

                        // 속성 설정
                        if !evaluated_props.is_empty() {
                            let prop_count = evaluated_props.len() as u64;
                            let tx_id = context.tx_id;
                            self.engine
                                .update_node_internal(tx_id, node_id, |node| {
                                    for (k, v) in evaluated_props.iter() {
                                        node.set_property(k, v.clone());
                                    }
                                })
                                .await?;
                            context.mutation_stats.inc_properties_set(prop_count);
                        }

                        // 변수 바인딩
                        if let Some(var) = &part.variable {
                            context.variables.insert(var.clone(), Value::Node(node_id));
                        }
                    }
                }
            }
            UpdatingClause::Delete { variables, detach } => {
                for var_name in variables {
                    let value = context.variables.get(var_name).ok_or_else(|| {
                        GraphError::Zeta(ZetaError::QueryExecution {
                            code: codes::cypher::VARIABLE_NOT_FOUND,
                            message: format!("Variable not found: {}", var_name),
                        })
                    })?;

                    match value {
                        Value::Node(node_id) => {
                            // DETACH DELETE: 연결된 관계도 삭제
                            if *detach {
                                let connected_edges = self
                                    .engine
                                    .get_connected_edges_in_transaction(context.tx_id, *node_id)
                                    .await?;
                                let edge_count = connected_edges.len() as u64;
                                context.mutation_stats.inc_relationships_deleted(edge_count);
                            }
                            // 노드 삭제
                            self.engine
                                .delete_node_internal(context.tx_id, *node_id, *detach)
                                .await?;
                            context.mutation_stats.inc_nodes_deleted(1);
                        }
                        Value::Edge(edge_id) => {
                            // 엣지 삭제
                            self.engine
                                .delete_edge_internal(context.tx_id, *edge_id)
                                .await?;
                            context.mutation_stats.inc_relationships_deleted(1);
                        }
                        _ => {
                            return Err(GraphError::Validation {
                                message: format!(
                                    "Cannot delete value: {} is {} (expected Node or Edge)",
                                    var_name,
                                    value.type_name()
                                ),
                            });
                        }
                    }
                }
            }
            UpdatingClause::Set { items } => {
                self.apply_set_items(items, context).await?;
            }
            UpdatingClause::Merge {
                pattern,
                on_create,
                on_match,
            } => {
                // MERGE: 존재하면 매치, 없으면 생성
                // 관계 패턴 지원 (Issue #71): (a)-[:REL]->(b) 형태
                if pattern.elements.len() >= 3 {
                    // 관계 패턴 MERGE
                    self.execute_merge_relationship_pattern(pattern, on_create, on_match, context)
                        .await?;
                } else if let Some(part) = pattern.parts.first() {
                    // 단순 노드 MERGE (기존 로직)
                    let matched_node = self.find_matching_node(part, context).await?;

                    match matched_node {
                        Some(node_id) => {
                            // 노드가 존재함 -> 변수 바인딩 + ON MATCH SET 실행
                            if let Some(var) = &part.variable {
                                context.variables.insert(var.clone(), Value::Node(node_id));
                            }
                            if let Some(items) = on_match {
                                self.apply_set_items(items, context).await?;
                            }
                        }
                        None => {
                            // 노드가 없음 -> 생성 + ON CREATE SET 실행
                            let labels = part.labels.clone();
                            let node_id = self
                                .engine
                                .create_node_internal(context.tx_id, labels)
                                .await?;

                            // 패턴의 속성 설정 (Expression을 먼저 평가)
                            if let Some(props) = &part.properties {
                                let evaluated_props =
                                    self.evaluate_property_pattern(props, context).await?;
                                let tx_id = context.tx_id;
                                self.engine
                                    .update_node_internal(tx_id, node_id, |node| {
                                        for (k, v) in evaluated_props.iter() {
                                            node.set_property(k, v.clone());
                                        }
                                    })
                                    .await?;
                            }

                            // 변수 바인딩
                            if let Some(var) = &part.variable {
                                context.variables.insert(var.clone(), Value::Node(node_id));
                            }

                            // ON CREATE SET 실행
                            if let Some(items) = on_create {
                                self.apply_set_items(items, context).await?;
                            }
                        }
                    }
                }
            }
            UpdatingClause::Remove { items } => {
                for item in items {
                    match item {
                        RemoveItem::Property { variable, property } => {
                            let value = context.variables.get(variable).ok_or_else(|| {
                                GraphError::Zeta(ZetaError::QueryExecution {
                                    code: codes::cypher::VARIABLE_NOT_FOUND,
                                    message: format!("Variable not found: {}", variable),
                                })
                            })?;

                            let node_id = match value {
                                Value::Node(id) => *id,
                                Value::Integer(id) => *id as u64,
                                Value::Edge(edge_id) => {
                                    let tx_id = context.tx_id;
                                    self.engine
                                        .update_edge_internal(tx_id, *edge_id, |edge| {
                                            edge.remove_property(property);
                                        })
                                        .await?;
                                    context.mutation_stats.inc_properties_set(1);
                                    continue;
                                }
                                _ => {
                                    return Err(GraphError::Validation {
                                        message: format!(
                                            "Cannot remove property from non-node/edge: {}",
                                            variable
                                        ),
                                    });
                                }
                            };
                            let tx_id = context.tx_id;
                            self.engine
                                .update_node_internal(tx_id, node_id, |node| {
                                    node.remove_property(property);
                                })
                                .await?;
                            context.mutation_stats.inc_properties_set(1);
                        }
                        RemoveItem::Label { variable, labels } => {
                            let value = context.variables.get(variable).ok_or_else(|| {
                                GraphError::Zeta(ZetaError::QueryExecution {
                                    code: codes::cypher::VARIABLE_NOT_FOUND,
                                    message: format!("Variable not found: {}", variable),
                                })
                            })?;

                            let node_id = match value {
                                Value::Node(id) => *id,
                                Value::Integer(id) => *id as u64,
                                _ => {
                                    return Err(GraphError::Validation {
                                        message: format!(
                                            "Cannot remove labels from non-node: {}",
                                            variable
                                        ),
                                    });
                                }
                            };
                            let tx_id = context.tx_id;
                            for label in labels {
                                self.engine
                                    .update_node_internal(tx_id, node_id, |node| {
                                        node.remove_label(label);
                                    })
                                    .await?;
                            }
                            context
                                .mutation_stats
                                .inc_labels_removed(labels.len() as u64);
                        }
                    }
                }
            }
            UpdatingClause::Foreach {
                variable,
                list,
                updates,
            } => {
                // FOREACH: 리스트의 각 요소에 대해 업데이트 실행
                let list_value = self.evaluate_expression(list, context).await?;

                let items = match list_value {
                    Value::Array(items) => items,
                    Value::Null => return Ok(()), // NULL 리스트는 무시
                    other => {
                        return Err(GraphError::Validation {
                            message: format!("FOREACH requires a list, got {}", other.type_name()),
                        });
                    }
                };

                // 각 요소에 대해 반복
                for item in items {
                    // 반복 변수를 새 값으로 바인딩
                    context.variables.insert(variable.clone(), item);

                    // 각 업데이트 절 실행
                    for update in updates {
                        self.execute_updating(update, context).await?;
                    }
                }

                // 반복 변수 제거 (옵션: 범위 밖으로 나감)
                context.variables.shift_remove(variable);
            }
        }
        Ok(())
    }

    /// CREATE 경로 패턴 실행: (a:Label {props})-[:REL]->(b:Label {props})
    /// 노드가 이미 바인딩되어 있으면 재사용, 없으면 새로 생성
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn execute_create_path(
        &self,
        pattern: &Pattern,
        context: &mut ExecutionContext,
    ) -> Result<()> {
        let elements = &pattern.elements;
        if elements.is_empty() {
            return Ok(());
        }

        // 첫 번째는 반드시 노드여야 함
        let mut prev_node_id: Option<u64> = None;

        let mut i = 0;
        while i < elements.len() {
            match &elements[i] {
                PatternElement::Node(node_pattern) => {
                    // 변수가 이미 바인딩되어 있으면 해당 노드 사용
                    let node_id = if let Some(var) = &node_pattern.variable {
                        if let Some(value) = context.variables.get(var) {
                            // 이미 바인딩된 변수 사용
                            match value {
                                Value::Node(id) => *id,
                                Value::Integer(id) => *id as u64,
                                _ => {
                                    return Err(GraphError::Validation {
                                        message: format!("Variable {} is not a node", var),
                                    });
                                }
                            }
                        } else {
                            // 새 노드 생성
                            let labels = node_pattern.labels.clone();
                            let label_count = labels.len() as u64;

                            // 속성 먼저 평가 (노드 생성 전에 검증을 위해)
                            let mut props_map = if let Some(props) = &node_pattern.properties {
                                self.evaluate_property_pattern(props, context).await?
                            } else {
                                IndexMap::new()
                            };

                            // Default 제약조건 적용 (검증 전에)
                            self.apply_default_constraints(&labels, &mut props_map)?;

                            // 노드 생성 (검증 통과 후에만 실행됨)
                            let new_node_id = self
                                .engine
                                .create_node_internal(context.tx_id, labels.clone())
                                .await?;

                            // 노드 생성 통계
                            context.mutation_stats.inc_nodes_created(1);
                            context.mutation_stats.inc_labels_added(label_count);

                            // 속성 설정
                            if !props_map.is_empty() {
                                let prop_count = props_map.len() as u64;
                                let tx_id = context.tx_id;
                                self.engine
                                    .update_node_internal(tx_id, new_node_id, |node| {
                                        for (k, v) in props_map.iter() {
                                            node.set_property(k, v.clone());
                                        }
                                    })
                                    .await?;
                                context.mutation_stats.inc_properties_set(prop_count);
                            }

                            // 제약조건 검증 (WARN 모드 처리)
                            self.validate_node_constraints(new_node_id, &labels, &props_map)
                                .await?;

                            // 변수 바인딩
                            context
                                .variables
                                .insert(var.clone(), Value::Node(new_node_id));
                            new_node_id
                        }
                    } else {
                        // 변수 없음 - 노드 생성만
                        let labels = node_pattern.labels.clone();
                        let label_count = labels.len() as u64;

                        // 속성 먼저 평가 (노드 생성 전에 검증을 위해)
                        let mut props_map = if let Some(props) = &node_pattern.properties {
                            self.evaluate_property_pattern(props, context).await?
                        } else {
                            IndexMap::new()
                        };

                        // Default 제약조건 적용 (검증 전에)
                        self.apply_default_constraints(&labels, &mut props_map)?;

                        // 노드 생성 (검증 통과 후에만 실행됨)
                        let new_node_id = self
                            .engine
                            .create_node_internal(context.tx_id, labels.clone())
                            .await?;

                        // 노드 생성 통계
                        context.mutation_stats.inc_nodes_created(1);
                        context.mutation_stats.inc_labels_added(label_count);

                        // 속성 설정
                        if !props_map.is_empty() {
                            let prop_count = props_map.len() as u64;
                            let tx_id = context.tx_id;
                            self.engine
                                .update_node_internal(tx_id, new_node_id, |node| {
                                    for (k, v) in props_map.iter() {
                                        node.set_property(k, v.clone());
                                    }
                                })
                                .await?;
                            context.mutation_stats.inc_properties_set(prop_count);
                        }

                        // 제약조건 검증 (WARN 모드 처리)
                        self.validate_node_constraints(new_node_id, &labels, &props_map)
                            .await?;

                        new_node_id
                    };

                    prev_node_id = Some(node_id);
                }
                PatternElement::Edge(edge_pattern) => {
                    // 엣지 처리: 다음 요소는 노드여야 함
                    let from_node = prev_node_id.ok_or_else(|| GraphError::Validation {
                        message: "Edge must follow a node in pattern".to_string(),
                    })?;

                    // 다음 노드 패턴 가져오기
                    i += 1;
                    let next_node_pattern = match elements.get(i) {
                        Some(PatternElement::Node(np)) => np,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "Edge must be followed by a node".to_string(),
                            });
                        }
                    };

                    // 다음 노드 생성 또는 바인딩
                    let to_node = if let Some(var) = &next_node_pattern.variable {
                        if let Some(value) = context.variables.get(var) {
                            match value {
                                Value::Node(id) => *id,
                                Value::Integer(id) => *id as u64,
                                _ => {
                                    return Err(GraphError::Validation {
                                        message: format!("Variable {} is not a node", var),
                                    });
                                }
                            }
                        } else {
                            let labels = next_node_pattern.labels.clone();
                            let label_count = labels.len() as u64;
                            let new_node_id = self
                                .engine
                                .create_node_internal(context.tx_id, labels.clone())
                                .await?;

                            // 노드 생성 통계
                            context.mutation_stats.inc_nodes_created(1);
                            context.mutation_stats.inc_labels_added(label_count);

                            // 속성 설정 (Expression을 먼저 평가)
                            let mut props_map = if let Some(props) = &next_node_pattern.properties {
                                let evaluated_props =
                                    self.evaluate_property_pattern(props, context).await?;
                                let prop_count = evaluated_props.len() as u64;
                                let tx_id = context.tx_id;
                                self.engine
                                    .update_node_internal(tx_id, new_node_id, |node| {
                                        for (k, v) in evaluated_props.iter() {
                                            node.set_property(k, v.clone());
                                        }
                                    })
                                    .await?;
                                context.mutation_stats.inc_properties_set(prop_count);
                                evaluated_props
                            } else {
                                IndexMap::new()
                            };

                            // Default 제약조건 적용
                            let original_len = props_map.len();
                            self.apply_default_constraints(&labels, &mut props_map)?;
                            if props_map.len() > original_len {
                                let tx_id = context.tx_id;
                                self.engine
                                    .update_node_internal(tx_id, new_node_id, |node| {
                                        for (k, v) in props_map.iter() {
                                            if !node.properties().contains_key(k) {
                                                node.set_property(k, v.clone());
                                            }
                                        }
                                    })
                                    .await?;
                            }

                            // 제약조건 검증
                            self.validate_node_constraints(new_node_id, &labels, &props_map)
                                .await?;

                            context
                                .variables
                                .insert(var.clone(), Value::Node(new_node_id));
                            new_node_id
                        }
                    } else {
                        let labels = next_node_pattern.labels.clone();
                        let label_count = labels.len() as u64;
                        let new_node_id = self
                            .engine
                            .create_node_internal(context.tx_id, labels.clone())
                            .await?;

                        // 노드 생성 통계
                        context.mutation_stats.inc_nodes_created(1);
                        context.mutation_stats.inc_labels_added(label_count);

                        // 속성 설정 (Expression을 먼저 평가)
                        let mut props_map = if let Some(props) = &next_node_pattern.properties {
                            let evaluated_props =
                                self.evaluate_property_pattern(props, context).await?;
                            let prop_count = evaluated_props.len() as u64;
                            let tx_id = context.tx_id;
                            self.engine
                                .update_node_internal(tx_id, new_node_id, |node| {
                                    for (k, v) in evaluated_props.iter() {
                                        node.set_property(k, v.clone());
                                    }
                                })
                                .await?;
                            context.mutation_stats.inc_properties_set(prop_count);
                            evaluated_props
                        } else {
                            IndexMap::new()
                        };

                        // Default 제약조건 적용
                        let original_len = props_map.len();
                        self.apply_default_constraints(&labels, &mut props_map)?;
                        if props_map.len() > original_len {
                            let tx_id = context.tx_id;
                            self.engine
                                .update_node_internal(tx_id, new_node_id, |node| {
                                    for (k, v) in props_map.iter() {
                                        if !node.properties().contains_key(k) {
                                            node.set_property(k, v.clone());
                                        }
                                    }
                                })
                                .await?;
                        }

                        // 제약조건 검증
                        self.validate_node_constraints(new_node_id, &labels, &props_map)
                            .await?;

                        new_node_id
                    };

                    // 엣지 생성
                    let rel_type = edge_pattern
                        .rel_types
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "RELATED".to_string());

                    // 방향에 따른 엣지 생성
                    let (source, target) = match edge_pattern.direction {
                        EdgeDirection::Outgoing | EdgeDirection::Both => (from_node, to_node),
                        EdgeDirection::Incoming => (to_node, from_node),
                    };

                    // 속성 먼저 평가 (엣지 생성 전에 검증을 위해)
                    let edge_props = if let Some(props) = &edge_pattern.properties {
                        self.evaluate_property_pattern(props, context).await?
                    } else {
                        IndexMap::new()
                    };

                    // 엣지 생성 (검증 통과 후에만 실행됨)
                    let edge_id = self
                        .engine
                        .create_edge_internal(context.tx_id, rel_type.clone(), source, target)
                        .await?;

                    // 관계 생성 통계
                    context.mutation_stats.inc_relationships_created(1);

                    // 속성 설정
                    if !edge_props.is_empty() {
                        let prop_count = edge_props.len() as u64;
                        let props_clone = edge_props.clone();
                        self.engine
                            .update_edge_internal(context.tx_id, edge_id, |edge| {
                                for (key, value) in props_clone {
                                    edge.set_property(key, value);
                                }
                            })
                            .await?;
                        context.mutation_stats.inc_properties_set(prop_count);
                    }

                    // 엣지 변수 바인딩
                    if let Some(var) = &edge_pattern.variable {
                        context.variables.insert(var.clone(), Value::Edge(edge_id));
                    }

                    prev_node_id = Some(to_node);
                }
            }
            i += 1;
        }

        Ok(())
    }

    /// SET 항목들 적용
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn apply_set_items(
        &self,
        items: &[SetItem],
        context: &mut ExecutionContext,
    ) -> Result<()> {
        for item in items {
            match item {
                SetItem::Property {
                    variable,
                    property,
                    value,
                } => {
                    let node_id = self.get_node_id_from_variable(variable, context)?;
                    let evaluated_value = self.evaluate_expression(value, context).await?;
                    let prop_name = property.clone();

                    self.engine
                        .update_node_internal(context.tx_id, node_id, move |node| {
                            node.set_property(&prop_name, evaluated_value.clone());
                        })
                        .await?;
                    context.mutation_stats.inc_properties_set(1);
                }
                SetItem::AllProperties {
                    variable,
                    properties,
                } => {
                    // n = {props} - 기존 속성 모두 제거하고 새로 설정
                    let node_id = self.get_node_id_from_variable(variable, context)?;
                    let props_map = self.evaluate_map_expression(properties, context).await?;
                    let prop_count = props_map.len() as u64;

                    self.engine
                        .update_node_internal(context.tx_id, node_id, move |node| {
                            // 기존 속성 모두 제거
                            node.properties_mut().clear();
                            // 새 속성 설정
                            for (key, value) in props_map {
                                node.set_property(&key, value);
                            }
                        })
                        .await?;
                    context.mutation_stats.inc_properties_set(prop_count);
                }
                SetItem::MergeProperties {
                    variable,
                    properties,
                } => {
                    // n += {props} - 기존 속성 유지하고 새 속성 병합
                    let node_id = self.get_node_id_from_variable(variable, context)?;
                    let props_map = self.evaluate_map_expression(properties, context).await?;
                    let prop_count = props_map.len() as u64;

                    self.engine
                        .update_node_internal(context.tx_id, node_id, move |node| {
                            // 새 속성만 추가/업데이트
                            for (key, value) in props_map {
                                node.set_property(&key, value);
                            }
                        })
                        .await?;
                    context.mutation_stats.inc_properties_set(prop_count);
                }
                SetItem::AddLabel { variable, labels } => {
                    // n:Label - 레이블 추가
                    let node_id = self.get_node_id_from_variable(variable, context)?;
                    let label_count = labels.len() as u64;
                    let labels_clone = labels.clone();

                    self.engine
                        .update_node_internal(context.tx_id, node_id, move |node| {
                            for label in &labels_clone {
                                node.add_label(label.clone());
                            }
                        })
                        .await?;
                    context.mutation_stats.inc_labels_added(label_count);
                }
            }
        }
        Ok(())
    }

    /// 변수에서 노드 ID 추출
    pub(super) fn get_node_id_from_variable(
        &self,
        variable: &str,
        context: &ExecutionContext,
    ) -> Result<u64> {
        let node_val = context.variables.get(variable).ok_or_else(|| {
            GraphError::Zeta(ZetaError::QueryExecution {
                code: codes::cypher::VARIABLE_NOT_FOUND,
                message: format!("Variable not found: {}", variable),
            })
        })?;

        match node_val {
            Value::Node(id) => Ok(*id),
            _ => Err(GraphError::Validation {
                message: format!(
                    "Cannot access {}: expected Node, got {}",
                    variable,
                    node_val.type_name()
                ),
            }),
        }
    }

    /// 맵 표현식 평가
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn evaluate_map_expression(
        &self,
        expr: &Expression,
        context: &ExecutionContext,
    ) -> Result<Vec<(String, Value)>> {
        match expr {
            Expression::Map(pairs) => {
                let mut result = Vec::new();
                for (key, value_expr) in pairs {
                    let value = self.evaluate_expression(value_expr, context).await?;
                    result.push((key.clone(), value));
                }
                Ok(result)
            }
            _ => Err(GraphError::Validation {
                message: format!("Expected map expression, got {:?}", expr),
            }),
        }
    }

    /// 패턴과 일치하는 노드 찾기 (MERGE용)
    ///
    /// v1.12.0: async로 변경
    pub(super) async fn find_matching_node(
        &self,
        part: &PatternPart,
        context: &ExecutionContext,
    ) -> Result<Option<u64>> {
        // 1. 라벨로 후보 검색
        let candidates = if let Some(label) = part.labels.first() {
            self.engine
                .find_nodes_by_label_in_transaction(context.tx_id, label)
                .await?
        } else {
            return Ok(None);
        };

        // 2. 레이블 및 속성으로 필터링 (Expression을 평가하여 비교)
        for node_id in candidates {
            let node = self
                .engine
                .get_node_in_transaction(context.tx_id, node_id)
                .await?;
            if let Some(node) = node {
                // 모든 필수 레이블 검사
                let has_all_labels = part
                    .labels
                    .iter()
                    .all(|required| node.labels().iter().any(|l| l == required));
                if !has_all_labels {
                    continue;
                }

                let matched = if let Some(props) = &part.properties {
                    self.matches_property_pattern_for_merge(props, &node, context)
                        .await?
                } else {
                    true
                };
                if matched {
                    return Ok(Some(node_id));
                }
            }
        }

        Ok(None)
    }

    /// PropertyPattern의 프로퍼티가 노드와 일치하는지 검사 (MERGE용)
    ///
    /// v1.12.0: async로 변경
    async fn matches_property_pattern_for_merge(
        &self,
        props: &PropertyPattern,
        node: &Node,
        context: &ExecutionContext,
    ) -> Result<bool> {
        for (key, expr) in props.iter() {
            let expected_value = self.evaluate_expression(expr, context).await?;
            match node.get_property(key) {
                Some(actual_value) if *actual_value == expected_value => continue,
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    /// MERGE 관계 패턴 실행 (Issue #71)
    /// (a)-[:REL]->(b) 형태의 패턴에서 관계가 없으면 생성
    ///
    /// v1.12.0: async로 변경
    async fn execute_merge_relationship_pattern(
        &self,
        pattern: &Pattern,
        on_create: &Option<Vec<SetItem>>,
        on_match: &Option<Vec<SetItem>>,
        context: &mut ExecutionContext,
    ) -> Result<()> {
        let elements = &pattern.elements;

        // 최소 3개 요소: 노드-엣지-노드
        if elements.len() < 3 {
            return Ok(());
        }

        // 첫 번째 노드 처리
        let start_node_id = match &elements[0] {
            PatternElement::Node(node_pattern) => {
                self.resolve_or_create_merge_node(node_pattern, context)
                    .await?
            }
            _ => {
                return Err(GraphError::Validation {
                    message: "MERGE pattern must start with a node".to_string(),
                });
            }
        };

        let mut prev_node_id = start_node_id;
        let mut i = 1;
        let mut created_any = false;

        while i < elements.len() {
            // 엣지 패턴
            let edge_pattern = match &elements[i] {
                PatternElement::Edge(ep) => ep,
                _ => {
                    i += 1;
                    continue;
                }
            };

            i += 1;

            // 다음 노드 패턴
            let next_node_pattern = match elements.get(i) {
                Some(PatternElement::Node(np)) => np,
                _ => {
                    return Err(GraphError::Validation {
                        message: "Edge must be followed by a node in MERGE".to_string(),
                    });
                }
            };

            // 다음 노드 해결
            let next_node_id = self
                .resolve_or_create_merge_node(next_node_pattern, context)
                .await?;

            // 관계 타입
            let rel_type = edge_pattern
                .rel_types
                .first()
                .cloned()
                .unwrap_or_else(|| "RELATED".to_string());

            // 방향에 따른 소스/타겟 결정
            let (source, target) = match edge_pattern.direction {
                EdgeDirection::Outgoing | EdgeDirection::Both => (prev_node_id, next_node_id),
                EdgeDirection::Incoming => (next_node_id, prev_node_id),
            };

            // 관계 존재 여부 확인
            let existing_edge = self
                .find_existing_edge(source, target, &rel_type, edge_pattern, context)
                .await?;

            match existing_edge {
                Some(edge_id) => {
                    // 관계 존재 - 변수 바인딩 + ON MATCH
                    if let Some(var) = &edge_pattern.variable {
                        context.variables.insert(var.clone(), Value::Edge(edge_id));
                    }
                    if let Some(items) = on_match {
                        self.apply_set_items(items, context).await?;
                    }
                }
                None => {
                    // 관계 없음 - 생성 + ON CREATE
                    // 속성 먼저 평가 (엣지 생성 전에 검증을 위해)
                    let edge_props = if let Some(props) = &edge_pattern.properties {
                        self.evaluate_property_pattern(props, context).await?
                    } else {
                        IndexMap::new()
                    };

                    // 엣지 생성 (검증 통과 후에만 실행됨)
                    let edge_id = self
                        .engine
                        .create_edge_internal(context.tx_id, rel_type, source, target)
                        .await?;

                    // 속성 설정
                    if !edge_props.is_empty() {
                        let props_clone = edge_props.clone();
                        self.engine
                            .update_edge_internal(context.tx_id, edge_id, |edge| {
                                for (key, value) in props_clone {
                                    edge.set_property(key, value);
                                }
                            })
                            .await?;
                    }

                    // 변수 바인딩
                    if let Some(var) = &edge_pattern.variable {
                        context.variables.insert(var.clone(), Value::Edge(edge_id));
                    }

                    created_any = true;
                }
            }

            prev_node_id = next_node_id;
            i += 1;
        }

        // ON CREATE/ON MATCH는 관계 생성 여부에 따라 한 번만 실행
        if created_any && let Some(items) = on_create {
            self.apply_set_items(items, context).await?;
        }

        Ok(())
    }

    /// MERGE용 노드 해결: 변수가 바인딩되어 있으면 사용, 아니면 MERGE 로직
    ///
    /// v1.12.0: async로 변경
    async fn resolve_or_create_merge_node(
        &self,
        node_pattern: &NodePattern,
        context: &mut ExecutionContext,
    ) -> Result<u64> {
        // 변수가 이미 바인딩되어 있으면 사용
        if let Some(var) = &node_pattern.variable
            && let Some(value) = context.variables.get(var)
        {
            return match value {
                Value::Node(id) => Ok(*id),
                Value::Integer(id) => Ok(*id as u64),
                _ => Err(GraphError::Validation {
                    message: format!("Variable {} is not a node", var),
                }),
            };
        }

        // 기존 노드 찾기
        if let Some(node_id) = self.find_matching_node(node_pattern, context).await? {
            // 변수 바인딩
            if let Some(var) = &node_pattern.variable {
                context.variables.insert(var.clone(), Value::Node(node_id));
            }
            return Ok(node_id);
        }

        // 노드 생성
        let labels = node_pattern.labels.clone();
        let node_id = self
            .engine
            .create_node_internal(context.tx_id, labels)
            .await?;

        // 속성 설정
        if let Some(props) = &node_pattern.properties {
            let evaluated_props = self.evaluate_property_pattern(props, context).await?;
            self.engine
                .update_node_internal(context.tx_id, node_id, |node| {
                    for (k, v) in evaluated_props.iter() {
                        node.set_property(k, v.clone());
                    }
                })
                .await?;
        }

        // 변수 바인딩
        if let Some(var) = &node_pattern.variable {
            context.variables.insert(var.clone(), Value::Node(node_id));
        }

        Ok(node_id)
    }

    /// 특정 소스-타겟 간 관계 존재 여부 확인
    /// v1.12.0: async로 변경
    async fn find_existing_edge(
        &self,
        source: u64,
        target: u64,
        rel_type: &str,
        edge_pattern: &EdgePattern,
        context: &ExecutionContext,
    ) -> Result<Option<u64>> {
        // 소스 노드에서 나가는 엣지 조회
        let edges = self
            .engine
            .get_connected_edges_in_transaction(context.tx_id, source)
            .await?;

        for edge_id in edges {
            if let Some(edge) = self
                .engine
                .get_edge_in_transaction(context.tx_id, edge_id)
                .await?
            {
                // 타겟과 타입 확인 (from_node가 source이고 to_node가 target인지)
                if edge.from_node() == source
                    && edge.to_node() == target
                    && edge.relation_type() == rel_type
                {
                    // 속성 패턴이 있으면 확인
                    if let Some(props) = &edge_pattern.properties {
                        let mut matches = true;
                        for (key, expr) in props.iter() {
                            let expected = self.evaluate_expression(expr, context).await?;
                            match edge.get_property(key) {
                                Some(actual) if *actual == expected => continue,
                                _ => {
                                    matches = false;
                                    break;
                                }
                            }
                        }
                        if matches {
                            return Ok(Some(edge_id));
                        }
                    } else {
                        return Ok(Some(edge_id));
                    }
                }
            }
        }

        Ok(None)
    }

    // ========================================================================
    // 추론 적용 (InferenceApplier 연동)
    // ========================================================================
}
