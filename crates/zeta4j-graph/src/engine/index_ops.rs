//! GraphEngine Secondary Index 연산

use indexmap::IndexMap;

use crate::core::{NodeId, PropertyContainer, Value};
use crate::error::{GraphError, Result};
use crate::index::composite::CompositeIndexDef;
use crate::index::{FunctionExpression, FunctionIndexInfo, FunctionIndexStats};
use crate::storage::memory::{MemoryCategory, MemoryReservation};

use super::core::GraphEngine;
use super::{GraphIndexDefinition, GraphIndexKind, GraphTransactionDelta};

impl GraphEngine {
    fn prepare_index_definition_memory(
        &self,
        definition: &GraphIndexDefinition,
    ) -> Result<Option<MemoryReservation>> {
        if matches!(
            definition.kind,
            GraphIndexKind::Fulltext { .. } | GraphIndexKind::Vector { .. }
        ) {
            return Ok(None);
        }
        let nodes = self.committed_nodes.read();
        let headroom = Self::index_build_headroom(std::slice::from_ref(definition), &nodes);
        drop(nodes);
        self.try_reserve_memory(MemoryCategory::Index, headroom)
    }

    fn create_index_definition_and_projection(
        &self,
        definition: GraphIndexDefinition,
    ) -> Result<()> {
        definition.validate()?;
        let _ddl = self.index_definition_lock.lock();
        let mut headroom = self.prepare_index_definition_memory(&definition)?;
        self.canonical_store
            .create_index_definition(definition.clone())?;
        if let Err(error) = self.install_index_projection(&definition) {
            if let Err(rollback_error) =
                self.canonical_store.drop_index_definition(&definition.name)
            {
                return Err(GraphError::System {
                    message: format!(
                        "index '{}' projection failed ({error}); canonical rollback failed ({rollback_error})",
                        definition.name
                    ),
                });
            }
            return Err(error);
        }
        self.commit_index_memory(&mut headroom);
        Ok(())
    }

    /// Build the only fallible part of the derived node-index projection before
    /// canonical persistence. The prepared manager is swapped into visibility
    /// after commit without allocating or validating index entries.
    pub(crate) fn prepare_composite_index_projection(
        &self,
        delta: &GraphTransactionDelta,
    ) -> Result<crate::index::CompositeIndexManager> {
        let changed_node_ids = delta
            .delete_node_ids
            .iter()
            .copied()
            .chain(delta.upsert_nodes.iter().map(crate::core::Node::id))
            .collect::<std::collections::BTreeSet<_>>();
        let old_nodes = {
            let nodes = self.committed_nodes.read();
            changed_node_ids
                .iter()
                .filter_map(|node_id| nodes.get(node_id).cloned())
                .collect::<Vec<_>>()
        };
        let mut projection = self.composite_index_manager.read().clone();
        for node in old_nodes {
            let properties = node.properties().as_map();
            for label in node.labels() {
                projection.on_node_delete(label, node.id(), properties)?;
            }
        }
        for node in &delta.upsert_nodes {
            let properties = node.properties().as_map();
            for label in node.labels() {
                projection.on_node_insert(label, node.id(), properties)?;
            }
        }
        Ok(projection)
    }

    /// Publish infallible in-memory projections after canonical persistence.
    /// The composite projection was already validated and built by
    /// [`Self::prepare_composite_index_projection`].
    pub(crate) fn publish_node_index_projection_delta(
        &self,
        old_nodes: &[crate::core::Node],
        new_nodes: &[crate::core::Node],
    ) {
        for node in old_nodes {
            let labels: Vec<_> = node.labels().iter().cloned().collect();
            self.secondary_index_manager
                .unindex_node(node.id(), &labels);
            self.function_index_manager.unindex_node(node.id(), &labels);
            self.spatial_index_manager.on_node_deleted(node.id());
        }
        for node in new_nodes {
            let labels: Vec<_> = node.labels().iter().cloned().collect();
            let properties = node.properties().as_map();
            self.secondary_index_manager
                .index_node(node.id(), &labels, properties);
            self.function_index_manager
                .index_node(node.id(), &labels, properties);
            self.spatial_index_manager
                .on_node_created(node.id(), &labels, properties);
        }
    }

    pub(crate) fn restore_canonical_index_definitions(
        &self,
        definitions: Vec<GraphIndexDefinition>,
    ) -> Result<()> {
        for definition in definitions {
            definition.validate()?;
            self.install_index_projection(&definition)?;
        }
        Ok(())
    }

    fn install_index_projection(&self, definition: &GraphIndexDefinition) -> Result<()> {
        match &definition.kind {
            GraphIndexKind::Property => {
                let property = &definition.properties[0];
                if !self.secondary_index_manager.create_index(
                    &definition.name,
                    &definition.label,
                    property,
                ) {
                    return Err(GraphError::Index {
                        message: format!("Index '{}' already exists", definition.name),
                    });
                }
                self.rebuild_secondary_index(&definition.label, property)
            }
            GraphIndexKind::Composite => {
                let composite = CompositeIndexDef {
                    name: definition.name.clone(),
                    label: definition.label.clone(),
                    properties: definition.properties.clone(),
                    unique: false,
                    created_at: 0,
                };
                self.composite_index_manager
                    .write()
                    .create_index(composite)?;
                self.rebuild_composite_index(
                    &definition.name,
                    &definition.label,
                    &definition.properties,
                )
            }
            GraphIndexKind::Spatial => {
                let property = &definition.properties[0];
                self.spatial_index_manager
                    .create_index(&definition.label, property)
                    .map_err(|message| GraphError::Index { message })?;
                self.rebuild_spatial_index(&definition.label, property)
            }
            GraphIndexKind::Function { function } => {
                let property = &definition.properties[0];
                let expression =
                    FunctionExpression::parse(function, property).ok_or_else(|| {
                        GraphError::Index {
                            message: format!("unsupported index function '{function}'"),
                        }
                    })?;
                if !self.function_index_manager.create_index(
                    &definition.name,
                    &definition.label,
                    expression.clone(),
                ) {
                    return Err(GraphError::Index {
                        message: format!("Function index '{}' already exists", definition.name),
                    });
                }
                self.rebuild_function_index(&definition.name, &definition.label, &expression)
            }
            GraphIndexKind::Fulltext { .. } | GraphIndexKind::Vector { .. } => Ok(()),
        }
    }

    fn committed_node_ids_for_label(&self, label: &str) -> Vec<NodeId> {
        let mut node_ids: Vec<_> = self
            .committed_nodes
            .read()
            .values()
            .filter(|node| node.labels().contains(label))
            .map(crate::core::Node::id)
            .collect();
        node_ids.sort_unstable();
        node_ids
    }

    // ========== 프로퍼티 인덱스 관리 ==========

    /// 프로퍼티 인덱스 생성
    ///
    /// # Arguments
    /// * `name` - 인덱스 이름
    /// * `label` - 대상 노드 라벨
    /// * `properties` - 인덱싱할 프로퍼티 목록 (단일 또는 복합)
    ///
    /// # Errors
    /// - 이미 동일한 이름의 인덱스가 존재하는 경우
    pub async fn create_property_index(
        &self,
        name: &str,
        label: &str,
        properties: &[String],
    ) -> Result<()> {
        if properties.is_empty() {
            return Err(GraphError::Index {
                message: "At least one property is required".to_string(),
            });
        }

        let definition = GraphIndexDefinition {
            name: name.to_string(),
            label: label.to_string(),
            properties: properties.to_vec(),
            kind: if properties.len() == 1 {
                GraphIndexKind::Property
            } else {
                GraphIndexKind::Composite
            },
        };
        self.create_index_definition_and_projection(definition)
    }

    /// 기존 노드들을 복합 인덱스에 추가 (인덱스 생성 시)
    fn rebuild_composite_index(
        &self,
        _name: &str,
        label: &str,
        properties: &[String],
    ) -> Result<()> {
        let node_ids = self.committed_node_ids_for_label(label);

        for node_id in node_ids {
            if let Some(node) = self.committed_nodes.read().get(&node_id).cloned() {
                let node_props = node.properties().as_map();

                // 모든 프로퍼티가 존재하는 경우에만 인덱싱
                let has_all_props = properties.iter().all(|p| node_props.contains_key(p));
                if has_all_props {
                    self.composite_index_manager
                        .write()
                        .on_node_insert(label, node_id, node_props)?;
                }
            }
        }

        Ok(())
    }

    /// 기존 노드들을 Secondary Index에 추가 (인덱스 생성 시)
    fn rebuild_secondary_index(&self, label: &str, property: &str) -> Result<()> {
        // 해당 라벨을 가진 모든 노드 조회
        let node_ids = self.committed_node_ids_for_label(label);

        for node_id in node_ids {
            if let Some(node) = self.committed_nodes.read().get(&node_id).cloned() {
                let labels: Vec<String> = node.labels().iter().cloned().collect();
                let properties: IndexMap<String, Value> = node.properties().as_map().clone();

                // 해당 프로퍼티가 있으면 인덱싱
                if properties.contains_key(property) {
                    self.secondary_index_manager
                        .index_node(node_id, &labels, &properties);
                }
            }
        }

        Ok(())
    }

    /// 프로퍼티 인덱스 삭제
    ///
    /// # Arguments
    /// * `name` - 삭제할 인덱스 이름
    ///
    /// # Errors
    /// - 인덱스가 존재하지 않는 경우
    pub fn drop_property_index(&self, name: &str) -> Result<()> {
        let _ddl = self.index_definition_lock.lock();
        let definition = self
            .canonical_store
            .index_definitions()?
            .into_iter()
            .find(|definition| definition.name == name)
            .ok_or_else(|| GraphError::Index {
                message: format!("Index '{name}' not found"),
            })?;
        if !matches!(
            definition.kind,
            GraphIndexKind::Property | GraphIndexKind::Composite
        ) {
            return Err(GraphError::Index {
                message: format!("Index '{name}' is not a property index"),
            });
        }
        self.canonical_store.drop_index_definition(name)?;
        match definition.kind {
            GraphIndexKind::Property => {
                if !self.secondary_index_manager.drop_index(name) {
                    return Err(GraphError::System {
                        message: format!("property index projection '{name}' is missing"),
                    });
                }
            }
            GraphIndexKind::Composite => {
                if !self.composite_index_manager.write().drop_index(name)? {
                    return Err(GraphError::System {
                        message: format!("composite index projection '{name}' is missing"),
                    });
                }
            }
            _ => {
                return Err(GraphError::System {
                    message: format!("index projection kind changed while dropping '{name}'"),
                });
            }
        }
        self.sync_index_memory_reservation()
    }

    /// 프로퍼티 인덱스 목록 조회
    ///
    /// # Returns
    /// Vec<(인덱스명, 라벨, 프로퍼티 목록)>
    pub fn list_property_indexes(&self) -> Vec<(String, String, Vec<String>)> {
        // 단일 프로퍼티 인덱스
        let mut result: Vec<(String, String, Vec<String>)> = self
            .secondary_index_manager
            .list_indexes()
            .into_iter()
            .map(|(name, label, prop, _count)| (name, label, vec![prop]))
            .collect();

        // 복합 인덱스 추가
        for def in self.composite_index_manager.read().list_indexes() {
            result.push((def.name.clone(), def.label.clone(), def.properties.clone()));
        }

        result
    }

    pub async fn create_fulltext_index(
        &self,
        name: &str,
        label: &str,
        properties: &[String],
        analyzer: &str,
    ) -> Result<()> {
        let definition = GraphIndexDefinition {
            name: name.to_string(),
            label: label.to_string(),
            properties: properties.to_vec(),
            kind: GraphIndexKind::Fulltext {
                analyzer: analyzer.to_string(),
            },
        };
        self.create_index_definition_and_projection(definition)
    }

    pub fn drop_fulltext_index(&self, name: &str) -> Result<()> {
        let _ddl = self.index_definition_lock.lock();
        let definition = self
            .canonical_store
            .index_definitions()?
            .into_iter()
            .find(|definition| definition.name == name)
            .ok_or_else(|| GraphError::Index {
                message: format!("Index '{name}' not found"),
            })?;
        if !matches!(definition.kind, GraphIndexKind::Fulltext { .. }) {
            return Err(GraphError::Index {
                message: format!("Index '{name}' is not a fulltext index"),
            });
        }
        self.canonical_store.drop_index_definition(name)?;
        Ok(())
    }

    pub fn list_fulltext_indexes(&self) -> Result<Vec<GraphIndexDefinition>> {
        let mut definitions = self
            .canonical_store
            .index_definitions()?
            .into_iter()
            .filter(|definition| matches!(definition.kind, GraphIndexKind::Fulltext { .. }))
            .collect::<Vec<_>>();
        definitions.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(definitions)
    }

    pub async fn create_vector_index(
        &self,
        name: &str,
        label: &str,
        property: &str,
        dimensions: u32,
        distance: &str,
    ) -> Result<()> {
        let definition = GraphIndexDefinition {
            name: name.to_string(),
            label: label.to_string(),
            properties: vec![property.to_string()],
            kind: GraphIndexKind::Vector {
                dimensions,
                distance: distance.to_string(),
            },
        };
        self.create_index_definition_and_projection(definition)
    }

    pub fn drop_vector_index(&self, name: &str) -> Result<()> {
        let _ddl = self.index_definition_lock.lock();
        let definition = self
            .canonical_store
            .index_definitions()?
            .into_iter()
            .find(|definition| definition.name == name)
            .ok_or_else(|| GraphError::Index {
                message: format!("Index '{name}' not found"),
            })?;
        if !matches!(definition.kind, GraphIndexKind::Vector { .. }) {
            return Err(GraphError::Index {
                message: format!("Index '{name}' is not a vector index"),
            });
        }
        self.canonical_store.drop_index_definition(name)?;
        Ok(())
    }

    pub fn list_vector_indexes(&self) -> Result<Vec<GraphIndexDefinition>> {
        let mut definitions = self
            .canonical_store
            .index_definitions()?
            .into_iter()
            .filter(|definition| matches!(definition.kind, GraphIndexKind::Vector { .. }))
            .collect::<Vec<_>>();
        definitions.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(definitions)
    }

    /// 특정 라벨과 프로퍼티에 대한 인덱스 존재 여부 확인
    pub fn has_property_index(&self, label: &str, property: &str) -> bool {
        self.secondary_index_manager.has_index_for(label, property)
    }

    /// 인덱스 이름으로 인덱스 정보 조회
    pub fn get_property_index(&self, name: &str) -> Option<(String, Vec<String>)> {
        // 단일 프로퍼티 인덱스 확인
        if let Some((label, prop)) = self.secondary_index_manager.get_index(name) {
            return Some((label, vec![prop]));
        }

        // 복합 인덱스 확인
        if let Some(index) = self.composite_index_manager.read().get_index(name) {
            let def = index.definition();
            return Some((def.label.clone(), def.properties.clone()));
        }

        None
    }

    /// Secondary Index를 사용하여 노드 검색
    ///
    /// # Arguments
    /// * `label` - 노드 라벨
    /// * `property` - 프로퍼티 이름
    /// * `value` - 검색할 값
    ///
    /// # Returns
    /// Some(노드 ID 목록) - 인덱스가 있고 검색 성공
    /// None - 해당 라벨+프로퍼티에 대한 인덱스가 없음
    pub fn find_nodes_by_index(
        &self,
        label: &str,
        property: &str,
        value: &Value,
    ) -> Option<Vec<NodeId>> {
        self.secondary_index_manager
            .find_nodes(label, property, value)
    }

    /// 범위 인덱스를 사용하여 노드 검색 (숫자)
    ///
    /// # Arguments
    /// * `label` - 노드 라벨 (필터링용)
    /// * `property` - 프로퍼티 이름
    /// * `min_value` - 최소값 (None이면 무한대)
    /// * `max_value` - 최대값 (None이면 무한대)
    /// * `min_inclusive` - 최소값 포함 여부
    /// * `max_inclusive` - 최대값 포함 여부
    ///
    /// # Returns
    /// 범위 내 노드 ID 목록
    pub async fn find_nodes_by_numeric_range(
        &self,
        label: &str,
        property: &str,
        min_value: Option<f64>,
        max_value: Option<f64>,
        min_inclusive: bool,
        max_inclusive: bool,
    ) -> Result<Vec<NodeId>> {
        // 범위 쿼리 실행
        let min = min_value.unwrap_or(f64::NEG_INFINITY);
        let max = max_value.unwrap_or(f64::INFINITY);

        let mut filtered = Vec::new();
        for node in self.committed_nodes.read().values() {
            let label_matches = label.is_empty() || node.labels().iter().any(|item| item == label);
            if label_matches
                && let Some(value) = node.get_property(property)
                && let Some(num) = value.as_float()
            {
                let min_ok = if min_inclusive { num >= min } else { num > min };
                let max_ok = if max_inclusive { num <= max } else { num < max };
                if min_ok && max_ok {
                    filtered.push(node.id());
                }
            }
        }
        filtered.sort_unstable();
        Ok(filtered)
    }

    /// 범위 인덱스를 사용하여 노드 검색 (문자열)
    ///
    /// # Arguments
    /// * `label` - 노드 라벨 (필터링용)
    /// * `property` - 프로퍼티 이름
    /// * `min_value` - 최소값 (None이면 빈 문자열)
    /// * `max_value` - 최대값 (None이면 무한대)
    /// * `min_inclusive` - 최소값 포함 여부
    /// * `max_inclusive` - 최대값 포함 여부
    ///
    /// # Returns
    /// 범위 내 노드 ID 목록
    pub async fn find_nodes_by_string_range(
        &self,
        label: &str,
        property: &str,
        min_value: Option<&str>,
        max_value: Option<&str>,
        min_inclusive: bool,
        max_inclusive: bool,
    ) -> Result<Vec<NodeId>> {
        // 범위 쿼리 실행
        let min = min_value.unwrap_or("");
        // 문자열 max는 0x10FFFF (유니코드 최대값) 기반 문자열 사용
        let max_default = "\u{10FFFF}";
        let max = max_value.unwrap_or(max_default);

        let mut filtered = Vec::new();
        for node in self.committed_nodes.read().values() {
            let label_matches = label.is_empty() || node.labels().iter().any(|item| item == label);
            if label_matches && let Some(Value::String(s)) = node.get_property(property) {
                let min_ok = if min_inclusive {
                    s.as_str() >= min
                } else {
                    s.as_str() > min
                };
                let max_ok = if max_inclusive {
                    s.as_str() <= max
                } else {
                    s.as_str() < max
                };
                if min_ok && max_ok {
                    filtered.push(node.id());
                }
            }
        }
        filtered.sort_unstable();
        Ok(filtered)
    }

    // ========== 공간 인덱스 관리 ==========

    /// 공간 인덱스 생성
    ///
    /// # Arguments
    /// * `label` - 대상 노드 라벨
    /// * `property` - 인덱싱할 공간 프로퍼티
    ///
    /// # Errors
    /// - 이미 동일한 인덱스가 존재하는 경우
    pub async fn create_spatial_index(&self, label: &str, property: &str) -> Result<()> {
        let definition = GraphIndexDefinition {
            name: GraphIndexDefinition::spatial_name(label, property),
            label: label.to_string(),
            properties: vec![property.to_string()],
            kind: GraphIndexKind::Spatial,
        };
        self.create_index_definition_and_projection(definition)
    }

    /// 기존 노드들을 Spatial Index에 추가 (인덱스 생성 시)
    fn rebuild_spatial_index(&self, label: &str, property: &str) -> Result<()> {
        // 해당 라벨을 가진 모든 노드 조회
        let node_ids = self.committed_node_ids_for_label(label);

        for node_id in node_ids {
            if let Some(node) = self.committed_nodes.read().get(&node_id).cloned() {
                let labels: Vec<String> = node.labels().iter().cloned().collect();
                let properties: IndexMap<String, Value> = node.properties().as_map().clone();

                // 해당 프로퍼티가 있으면 인덱싱
                if properties.contains_key(property) {
                    self.spatial_index_manager
                        .on_node_created(node_id, &labels, &properties);
                }
            }
        }

        Ok(())
    }

    /// 공간 인덱스 삭제
    ///
    /// # Arguments
    /// * `label` - 대상 노드 라벨
    /// * `property` - 공간 프로퍼티
    ///
    /// # Errors
    /// - 인덱스가 존재하지 않는 경우
    pub fn drop_spatial_index(&self, label: &str, property: &str) -> Result<()> {
        let _ddl = self.index_definition_lock.lock();
        let name = GraphIndexDefinition::spatial_name(label, property);
        self.canonical_store.drop_index_definition(&name)?;
        self.spatial_index_manager
            .drop_index(label, property)
            .map_err(|msg| GraphError::Index { message: msg })?;
        self.sync_index_memory_reservation()
    }

    /// 공간 인덱스 목록 조회
    ///
    /// # Returns
    /// Vec<(라벨, 프로퍼티)>
    pub fn list_spatial_indexes(&self) -> Vec<(String, String)> {
        self.spatial_index_manager.list_indexes()
    }

    /// 특정 라벨과 프로퍼티에 대한 공간 인덱스 존재 여부 확인
    pub fn has_spatial_index(&self, label: &str, property: &str) -> bool {
        self.spatial_index_manager.has_index(label, property)
    }

    // ========== 함수 인덱스 관리 ==========

    /// 함수 인덱스 생성
    ///
    /// # Arguments
    /// * `name` - 인덱스 이름
    /// * `label` - 대상 노드 라벨
    /// * `expression` - 함수 표현식 (예: `FunctionExpression::Lower("name".to_string())`)
    ///
    /// # Errors
    /// - 이미 동일한 이름의 인덱스가 존재하는 경우
    ///
    /// # Examples
    /// ```ignore
    /// use zeta4j_graph::index::FunctionExpression;
    ///
    /// // 대소문자 무시 검색을 위한 인덱스
    /// engine.create_function_index(
    ///     "idx_lower_name",
    ///     "Person",
    ///     FunctionExpression::Lower("name".to_string()),
    /// ).await?;
    ///
    /// // 연도 기반 검색을 위한 인덱스
    /// engine.create_function_index(
    ///     "idx_year",
    ///     "Event",
    ///     FunctionExpression::Year("date".to_string()),
    /// ).await?;
    /// ```
    pub async fn create_function_index(
        &self,
        name: &str,
        label: &str,
        expression: FunctionExpression,
    ) -> Result<()> {
        let definition = GraphIndexDefinition {
            name: name.to_string(),
            label: label.to_string(),
            properties: vec![expression.source_property().to_string()],
            kind: GraphIndexKind::Function {
                function: expression.function_name().to_string(),
            },
        };
        self.create_index_definition_and_projection(definition)
    }

    /// 기존 노드들을 Function Index에 추가 (인덱스 생성 시)
    fn rebuild_function_index(
        &self,
        _name: &str,
        label: &str,
        expression: &FunctionExpression,
    ) -> Result<()> {
        let property = expression.source_property();

        // 해당 라벨을 가진 모든 노드 조회
        let node_ids = self.committed_node_ids_for_label(label);

        for node_id in node_ids {
            if let Some(node) = self.committed_nodes.read().get(&node_id).cloned() {
                let labels: Vec<String> = node.labels().iter().cloned().collect();
                let properties: IndexMap<String, Value> = node.properties().as_map().clone();

                // 해당 프로퍼티가 있으면 인덱싱
                if properties.contains_key(property) {
                    self.function_index_manager
                        .index_node(node_id, &labels, &properties);
                }
            }
        }

        Ok(())
    }

    /// 함수 인덱스 삭제
    ///
    /// # Arguments
    /// * `name` - 삭제할 인덱스 이름
    ///
    /// # Errors
    /// - 인덱스가 존재하지 않는 경우
    pub fn drop_function_index(&self, name: &str) -> Result<()> {
        let _ddl = self.index_definition_lock.lock();
        let definition = self
            .canonical_store
            .index_definitions()?
            .into_iter()
            .find(|definition| definition.name == name)
            .ok_or_else(|| GraphError::Index {
                message: format!("Function index '{name}' not found"),
            })?;
        if !matches!(definition.kind, GraphIndexKind::Function { .. }) {
            return Err(GraphError::Index {
                message: format!("Index '{name}' is not a function index"),
            });
        }
        self.canonical_store.drop_index_definition(name)?;
        if !self.function_index_manager.drop_index(name) {
            return Err(GraphError::System {
                message: format!("function index projection '{name}' is missing"),
            });
        }

        self.sync_index_memory_reservation()
    }

    /// 함수 인덱스 목록 조회
    ///
    /// # Returns
    /// Vec<FunctionIndexInfo> - 인덱스 정보 목록
    pub fn list_function_indexes(&self) -> Vec<FunctionIndexInfo> {
        self.function_index_manager.list_indexes()
    }

    /// 특정 이름의 함수 인덱스 존재 여부 확인
    pub fn has_function_index(&self, name: &str) -> bool {
        self.function_index_manager.has_index(name)
    }

    /// 특정 함수 표현식에 대한 인덱스 찾기
    ///
    /// # Arguments
    /// * `label` - 노드 라벨
    /// * `expression` - 함수 표현식
    ///
    /// # Returns
    /// 일치하는 인덱스 이름 (있으면)
    pub fn find_function_index_for(
        &self,
        label: &str,
        expression: &FunctionExpression,
    ) -> Option<String> {
        self.function_index_manager
            .find_index_for(label, expression)
    }

    /// 함수 인덱스를 사용하여 노드 검색
    ///
    /// # Arguments
    /// * `label` - 노드 라벨
    /// * `expression` - 함수 표현식
    /// * `value` - 검색할 계산된 값
    ///
    /// # Returns
    /// Some(노드 ID 목록) - 인덱스가 있고 검색 성공
    /// None - 해당하는 인덱스가 없음
    ///
    /// # Examples
    /// ```ignore
    /// use zeta4j_graph::index::FunctionExpression;
    ///
    /// // lower(p.name) = 'alice' 검색
    /// let nodes = engine.find_nodes_by_function_index(
    ///     "Person",
    ///     &FunctionExpression::Lower("name".to_string()),
    ///     &Value::String("alice".to_string()),
    /// );
    /// ```
    pub fn find_nodes_by_function_index(
        &self,
        label: &str,
        expression: &FunctionExpression,
        value: &Value,
    ) -> Option<Vec<NodeId>> {
        self.function_index_manager
            .find_nodes(label, expression, value)
    }

    /// 인덱스 이름으로 노드 검색
    ///
    /// # Arguments
    /// * `index_name` - 인덱스 이름
    /// * `value` - 검색할 계산된 값
    ///
    /// # Returns
    /// Some(노드 ID 목록) - 인덱스가 있고 검색 성공
    /// None - 인덱스가 존재하지 않음
    pub fn find_nodes_by_function_index_name(
        &self,
        index_name: &str,
        value: &Value,
    ) -> Option<Vec<NodeId>> {
        self.function_index_manager
            .find_nodes_by_index_name(index_name, value)
    }

    /// 함수 인덱스 정보 조회
    pub fn get_function_index_info(&self, name: &str) -> Option<FunctionIndexInfo> {
        self.function_index_manager.get_index_info(name)
    }

    /// 함수 인덱스 통계 조회
    pub fn get_function_index_stats(&self) -> FunctionIndexStats {
        self.function_index_manager.get_stats()
    }

    /// 특정 라벨에 대한 모든 함수 인덱스 이름 조회
    pub fn function_indexes_for_label(&self, label: &str) -> Vec<String> {
        self.function_index_manager.indexes_for_label(label)
    }

    /// 특정 프로퍼티에 대한 모든 함수 인덱스 이름 조회
    pub fn function_indexes_for_property(&self, label: &str, property: &str) -> Vec<String> {
        self.function_index_manager
            .indexes_for_property(label, property)
    }

    /// 노드를 함수 인덱스에 수동으로 추가
    ///
    /// 노드 생성/업데이트 시 자동으로 인덱싱되지만,
    /// 테스트나 특수한 경우에 수동 인덱싱이 필요할 때 사용
    ///
    /// # Arguments
    /// * `node_id` - 인덱싱할 노드 ID
    /// * `labels` - 노드 라벨 목록
    /// * `properties` - 노드 프로퍼티
    pub fn index_node_for_function_indexes(
        &self,
        node_id: NodeId,
        labels: &[String],
        properties: &IndexMap<String, Value>,
    ) {
        self.function_index_manager
            .index_node(node_id, labels, properties);
    }

    /// 노드를 함수 인덱스에서 수동으로 제거
    ///
    /// # Arguments
    /// * `node_id` - 제거할 노드 ID
    /// * `labels` - 노드 라벨 목록
    pub fn unindex_node_for_function_indexes(&self, node_id: NodeId, labels: &[String]) {
        self.function_index_manager.unindex_node(node_id, labels);
    }

    // ========== 레이블 조회 ==========

    /// 그래프에 등록된 모든 노드 레이블 목록 조회
    ///
    /// GraphRAG 인덱싱 시 LLM이 동적으로 생성한 엔티티 타입(PERSON, ORGANIZATION 등)을
    /// 포함하여, 그래프에 존재하는 모든 레이블을 반환합니다.
    /// 검색 시 하드코딩된 레이블 목록 대신 이 메서드를 사용하세요.
    pub fn get_all_labels(&self) -> Vec<String> {
        let nodes = self.committed_nodes.read();
        let mut node_ids: Vec<_> = nodes.keys().copied().collect();
        node_ids.sort_unstable();
        let mut labels = indexmap::IndexSet::new();
        for node_id in node_ids {
            if let Some(node) = nodes.get(&node_id) {
                labels.extend(node.labels().iter().cloned());
            }
        }
        labels.into_iter().collect()
    }
}

#[cfg(test)]
mod memory_tests {
    use std::sync::Arc;

    use crate::engine::{CanonicalGraphStore, GraphEngineConfig, InMemoryCanonicalGraphStore};
    use crate::storage::memory::MemoryLimiterConfig;

    use super::*;

    #[tokio::test]
    async fn rejected_index_build_does_not_persist_definition_or_projection()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let store = Arc::new(InMemoryCanonicalGraphStore::default());
        let engine = GraphEngine::new(
            GraphEngineConfig::default().with_memory_limit(MemoryLimiterConfig {
                max_memory_bytes: 128,
                auto_cleanup_enabled: false,
                ..MemoryLimiterConfig::default()
            }),
            store.clone(),
        )
        .await?;

        let result = engine
            .create_property_index("person_name", "Person", &["name".to_string()])
            .await;
        assert!(matches!(result, Err(GraphError::Memory(_))));
        assert!(store.index_definitions()?.is_empty());
        assert!(engine.list_property_indexes().is_empty());
        Ok(())
    }
}
