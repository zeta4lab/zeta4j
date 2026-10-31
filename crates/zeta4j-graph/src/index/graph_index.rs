// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! GraphIndex - 통합 그래프 인덱싱 시스템
//!
//! 모든 인덱스를 통합 관리하여 일관성을 보장하고 복잡한 쿼리를 지원하는
//! 상위 레벨 인덱싱 시스템입니다.

use indexmap::IndexMap;

use crate::core::{Edge, EdgeId, Node, NodeId, PropertyContainer, Value};
use crate::error::{GraphError, Result};
use crate::index::{EdgeIndex, NodeIndex};
use parking_lot::RwLock;
use std::collections::HashMap;

/// 복합 인덱스 구조체
#[derive(Debug)]
pub struct CompositeIndex {
    /// 인덱스 이름
    name: String,
    /// 노드 라벨 조건 (옵션)
    label_condition: Option<String>,
    /// 속성 조건들 (key -> expected_value)
    property_conditions: IndexMap<String, Value>,
    /// 매칭되는 노드 ID들
    matching_nodes: RwLock<Vec<NodeId>>,
}

impl CompositeIndex {
    /// 인덱스 이름 조회
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// 범위 쿼리용 정렬된 인덱스
#[derive(Debug)]
pub struct RangeIndex {
    /// 숫자 값 인덱스 (정렬된)
    numeric_values: RwLock<Vec<(f64, Vec<NodeId>)>>,
    /// 문자열 값 인덱스 (정렬된)
    string_values: RwLock<Vec<(String, Vec<NodeId>)>>,
}

/// 통합 그래프 인덱싱 시스템
///
/// ## 성능 최적화 노트
///
/// `NodeIndex`와 `EdgeIndex`는 직접 소유로 관리됩니다.
/// 이전에는 `Arc`로 래핑되어 있었으나, `GraphIndex`가 단독으로
/// 소유하고 공유하지 않으므로 불필요한 오버헤드를 제거했습니다.
///
/// 이로 인한 이점:
/// - 메모리 할당 2회 감소 (Arc 자체의 힙 할당)
/// - 참조 카운팅 atomic 연산 제거
/// - 캐시 지역성 향상 (간접 참조 제거)
#[derive(Debug)]
pub struct GraphIndex {
    /// 노드 인덱싱 시스템
    node_index: NodeIndex,

    /// 엣지 인덱싱 시스템
    edge_index: EdgeIndex,

    /// 복합 인덱스 (여러 조건 결합)
    composite_indexes: RwLock<HashMap<String, CompositeIndex>>,

    /// 범위 쿼리용 B+ Tree 인덱스
    range_indexes: RwLock<HashMap<String, RangeIndex>>,

    /// 인덱스 통계
    stats: RwLock<IndexStats>,

    /// 레이블 순서 레지스트리 (처음 등록된 순서 추적)
    label_order: RwLock<IndexMap<String, u32>>,
}

/// 인덱스 통계 정보
#[derive(Debug, Default)]
pub struct IndexStats {
    /// 노드 인덱스 히트 수
    pub node_index_hits: u64,
    /// 엣지 인덱스 히트 수  
    pub edge_index_hits: u64,
    /// 복합 인덱스 히트 수
    pub composite_index_hits: u64,
    /// 범위 쿼리 히트 수
    pub range_query_hits: u64,
    /// 전체 쿼리 수
    pub total_queries: u64,
}

impl GraphIndex {
    /// 새로운 GraphIndex 생성
    pub fn new() -> Self {
        Self {
            node_index: NodeIndex::new(),
            edge_index: EdgeIndex::new(),
            composite_indexes: RwLock::new(HashMap::new()),
            range_indexes: RwLock::new(HashMap::new()),
            stats: RwLock::new(IndexStats::default()),
            label_order: RwLock::new(IndexMap::new()),
        }
    }

    /// 레이블 등록 (처음 등록 시 순서 할당)
    pub fn register_label(&self, label: &str) {
        let mut order = self.label_order.write();
        if !order.contains_key(label) {
            let next_id = order.len() as u32;
            order.insert(label.to_string(), next_id);
        }
    }

    /// 레이블 순서 조회 (미등록 시 u32::MAX 반환)
    pub fn get_label_order(&self, label: &str) -> u32 {
        self.label_order
            .read()
            .get(label)
            .copied()
            .unwrap_or(u32::MAX)
    }

    /// 등록된 모든 레이블 목록 반환
    pub fn get_all_labels(&self) -> Vec<String> {
        self.label_order.read().keys().cloned().collect()
    }

    /// 레이블 목록을 등록 순서대로 정렬
    pub fn sort_labels_by_order(&self, labels: &[String]) -> Vec<String> {
        let order = self.label_order.read();
        let mut sorted: Vec<_> = labels.to_vec();
        sorted.sort_by_key(|l| order.get(l).copied().unwrap_or(u32::MAX));
        sorted
    }

    /// 노드 인덱싱
    pub fn index_node(&self, node: &Node) -> Result<()> {
        // 1. 기본 인덱스 업데이트 - 라벨과 속성 별도로 추가
        for label in node.labels() {
            self.register_label(label); // 레이블 순서 등록
            self.node_index.add_node_label(node.id(), label);
        }

        for (key, value) in node.properties().iter() {
            self.node_index.add_node_property(node.id(), key, value);
        }

        // 2. 복합 인덱스 업데이트
        for composite in self.composite_indexes.read().values() {
            self.update_composite_index_for_node(composite, node)?;
        }

        // 3. 범위 인덱스 업데이트
        self.update_range_indexes_for_node(node)?;

        Ok(())
    }

    /// 엣지 인덱싱
    pub fn index_edge(&self, edge: &Edge) -> Result<()> {
        // 기본 엣지 인덱싱
        self.edge_index.add_edge(
            edge.id(),
            edge.relation_type(),
            edge.from_node(),
            edge.to_node(),
        );

        // 엣지 속성 인덱싱
        for (key, value) in edge.properties().iter() {
            self.edge_index.add_edge_property(edge.id(), key, value);
        }

        Ok(())
    }

    /// 노드 인덱스에서 제거
    pub fn unindex_node(&self, node: &Node) -> Result<()> {
        // 1. 기본 인덱스에서 제거 - 라벨과 속성 별도로 제거
        for label in node.labels() {
            self.node_index.remove_node_label(node.id(), label);
        }

        for (key, value) in node.properties().iter() {
            self.node_index.remove_node_property(node.id(), key, value);
        }

        // 2. 복합 인덱스에서 제거
        for composite in self.composite_indexes.read().values() {
            self.remove_from_composite_index(composite, node.id())?;
        }

        // 3. 범위 인덱스에서 제거
        self.remove_from_range_indexes(node)?;

        Ok(())
    }

    /// 엣지 인덱스에서 제거
    pub fn unindex_edge(&self, edge: &Edge) -> Result<()> {
        // 기본 인덱스에서 제거
        self.edge_index.remove_edge(
            edge.id(),
            edge.relation_type(),
            edge.from_node(),
            edge.to_node(),
        );

        // 속성 인덱스에서 제거
        for (key, value) in edge.properties().iter() {
            self.edge_index.remove_edge_property(edge.id(), key, value);
        }

        Ok(())
    }

    /// 노드 인덱스에서 제거 (별칭)
    pub fn remove_node(&self, node: &Node) -> Result<()> {
        self.unindex_node(node)
    }

    /// 엣지 인덱스에서 제거 (별칭)
    pub fn remove_edge(&self, edge: &Edge) -> Result<()> {
        self.unindex_edge(edge)
    }

    /// 라벨로 노드 검색
    pub fn find_nodes_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
        let mut stats = self.stats.write();
        stats.node_index_hits += 1;
        stats.total_queries += 1;
        drop(stats);

        Ok(self.node_index.find_nodes_by_label(label))
    }

    /// 속성으로 노드 검색
    pub fn find_nodes_by_property(&self, key: &str, value: &Value) -> Result<Vec<NodeId>> {
        let mut stats = self.stats.write();
        stats.node_index_hits += 1;
        stats.total_queries += 1;
        drop(stats);

        Ok(self.node_index.find_nodes_by_property(key, value))
    }

    /// 관계 타입으로 엣지 검색
    pub fn find_edges_by_type(&self, edge_type: &str) -> Result<Vec<EdgeId>> {
        let mut stats = self.stats.write();
        stats.edge_index_hits += 1;
        stats.total_queries += 1;
        drop(stats);

        Ok(self.edge_index.find_edges_by_relation_type(edge_type))
    }

    /// 노드 간 연결 검색
    pub fn find_edges_between(&self, source: NodeId, target: NodeId) -> Result<Vec<EdgeId>> {
        let mut stats = self.stats.write();
        stats.edge_index_hits += 1;
        stats.total_queries += 1;
        drop(stats);

        Ok(self.edge_index.find_edges_between_nodes(source, target))
    }

    /// 노드와 연결된 모든 엣지 검색 (양방향)
    pub fn find_edges_by_node(&self, node_id: NodeId) -> Result<Vec<EdgeId>> {
        let mut stats = self.stats.write();
        stats.edge_index_hits += 1;
        stats.total_queries += 1;
        drop(stats);

        Ok(self.edge_index.find_edges_by_node(node_id))
    }

    /// 특정 노드에서 특정 관계 타입으로 나가는 엣지들 검색
    pub fn find_outgoing_edges_by_type(
        &self,
        from_node: NodeId,
        rel_type: &str,
    ) -> Result<Vec<EdgeId>> {
        let mut stats = self.stats.write();
        stats.edge_index_hits += 1;
        stats.total_queries += 1;
        drop(stats);

        Ok(self
            .edge_index
            .find_outgoing_edges_by_type(from_node, rel_type))
    }

    /// 특정 노드로 특정 관계 타입으로 들어오는 엣지들 검색
    pub fn find_incoming_edges_by_type(
        &self,
        to_node: NodeId,
        rel_type: &str,
    ) -> Result<Vec<EdgeId>> {
        let mut stats = self.stats.write();
        stats.edge_index_hits += 1;
        stats.total_queries += 1;
        drop(stats);

        Ok(self
            .edge_index
            .find_incoming_edges_by_type(to_node, rel_type))
    }

    /// 복합 인덱스 생성
    pub fn create_composite_index(
        &self,
        name: String,
        label_condition: Option<String>,
        property_conditions: IndexMap<String, Value>,
    ) -> Result<()> {
        let composite = CompositeIndex {
            name: name.clone(),
            label_condition,
            property_conditions,
            matching_nodes: RwLock::new(Vec::new()),
        };

        self.composite_indexes.write().insert(name, composite);
        Ok(())
    }

    /// 복합 인덱스로 검색
    pub fn find_nodes_by_composite(&self, index_name: &str) -> Result<Vec<NodeId>> {
        let mut stats = self.stats.write();
        stats.composite_index_hits += 1;
        stats.total_queries += 1;

        let indexes = self.composite_indexes.read();
        if let Some(composite) = indexes.get(index_name) {
            Ok(composite.matching_nodes.read().clone())
        } else {
            Err(GraphError::Index {
                message: format!("Composite index '{}' not found", index_name),
            })
        }
    }

    /// 범위 쿼리 (숫자)
    pub fn find_nodes_in_numeric_range(
        &self,
        property_key: &str,
        min_value: f64,
        max_value: f64,
    ) -> Result<Vec<NodeId>> {
        let mut stats = self.stats.write();
        stats.range_query_hits += 1;
        stats.total_queries += 1;

        let indexes = self.range_indexes.read();
        if let Some(range_index) = indexes.get(property_key) {
            let values = range_index.numeric_values.read();
            let mut result = Vec::new();

            for (value, node_ids) in values.iter() {
                if *value >= min_value && *value <= max_value {
                    result.extend_from_slice(node_ids);
                }
            }

            Ok(result)
        } else {
            Ok(Vec::new())
        }
    }

    /// 범위 쿼리 (문자열)
    pub fn find_nodes_in_string_range(
        &self,
        property_key: &str,
        min_value: &str,
        max_value: &str,
    ) -> Result<Vec<NodeId>> {
        let mut stats = self.stats.write();
        stats.range_query_hits += 1;
        stats.total_queries += 1;

        let indexes = self.range_indexes.read();
        if let Some(range_index) = indexes.get(property_key) {
            let values = range_index.string_values.read();
            let mut result = Vec::new();

            for (value, node_ids) in values.iter() {
                if value.as_str() >= min_value && value.as_str() <= max_value {
                    result.extend_from_slice(node_ids);
                }
            }

            Ok(result)
        } else {
            Ok(Vec::new())
        }
    }

    /// 인덱스 통계 조회
    pub fn get_stats(&self) -> IndexStats {
        self.stats.read().clone()
    }

    /// 모든 인덱스 클리어
    pub fn clear(&self) -> Result<()> {
        self.node_index.clear();
        self.edge_index.clear();
        self.composite_indexes.write().clear();
        self.range_indexes.write().clear();
        *self.stats.write() = IndexStats::default();
        Ok(())
    }

    /// 인덱스 재구축 (온라인)
    pub fn rebuild_indexes(&self, nodes: &[Node], edges: &[Edge]) -> Result<()> {
        // 1. 기존 인덱스 클리어
        self.clear()?;

        // 2. 노드 인덱스 재구축
        for node in nodes {
            self.index_node(node)?;
        }

        // 3. 엣지 인덱스 재구축
        for edge in edges {
            self.index_edge(edge)?;
        }

        Ok(())
    }

    // === 헬퍼 메서드들 ===

    /// 복합 인덱스를 노드에 대해 업데이트
    fn update_composite_index_for_node(
        &self,
        composite: &CompositeIndex,
        node: &Node,
    ) -> Result<()> {
        let mut matches = true;

        // 라벨 조건 확인
        if let Some(required_label) = &composite.label_condition
            && !node.has_label(required_label)
        {
            matches = false;
        }

        // 속성 조건 확인
        if matches {
            for (key, expected_value) in &composite.property_conditions {
                if let Some(actual_value) = node.get_property(key) {
                    if actual_value != expected_value {
                        matches = false;
                        break;
                    }
                } else {
                    matches = false;
                    break;
                }
            }
        }

        // 매칭되는 경우 인덱스에 추가
        if matches {
            let mut matching_nodes = composite.matching_nodes.write();
            if !matching_nodes.contains(&node.id()) {
                matching_nodes.push(node.id());
            }
        }

        Ok(())
    }

    /// 복합 인덱스에서 노드 제거
    fn remove_from_composite_index(
        &self,
        composite: &CompositeIndex,
        node_id: NodeId,
    ) -> Result<()> {
        let mut matching_nodes = composite.matching_nodes.write();
        matching_nodes.retain(|&id| id != node_id);
        Ok(())
    }

    /// 범위 인덱스를 노드에 대해 업데이트
    fn update_range_indexes_for_node(&self, node: &Node) -> Result<()> {
        let mut indexes = self.range_indexes.write();

        for (key, value) in node.properties().iter() {
            // 해당 속성의 범위 인덱스가 없으면 생성
            let range_index = indexes.entry(key.clone()).or_insert_with(|| RangeIndex {
                numeric_values: RwLock::new(Vec::new()),
                string_values: RwLock::new(Vec::new()),
            });

            match value {
                Value::Integer(i) => {
                    let mut numeric_values = range_index.numeric_values.write();
                    self.insert_into_sorted_numeric(&mut numeric_values, *i as f64, node.id());
                }
                Value::Float(f) => {
                    let mut numeric_values = range_index.numeric_values.write();
                    self.insert_into_sorted_numeric(&mut numeric_values, *f, node.id());
                }
                Value::String(s) => {
                    let mut string_values = range_index.string_values.write();
                    self.insert_into_sorted_string(&mut string_values, s.clone(), node.id());
                }
                _ => {} // 다른 타입은 범위 쿼리 미지원
            }
        }

        Ok(())
    }

    /// 범위 인덱스에서 노드 제거
    fn remove_from_range_indexes(&self, node: &Node) -> Result<()> {
        let mut indexes = self.range_indexes.write();

        for (key, _) in node.properties().iter() {
            if let Some(range_index) = indexes.get_mut(key) {
                {
                    let mut numeric_values = range_index.numeric_values.write();
                    numeric_values.retain_mut(|(_, node_ids)| {
                        node_ids.retain(|&id| id != node.id());
                        !node_ids.is_empty()
                    });
                }

                {
                    let mut string_values = range_index.string_values.write();
                    string_values.retain_mut(|(_, node_ids)| {
                        node_ids.retain(|&id| id != node.id());
                        !node_ids.is_empty()
                    });
                }
            }
        }

        Ok(())
    }

    /// 정렬된 숫자 값에 삽입
    ///
    /// NaN 값은 비교가 불가능하므로 거부됩니다 (인덱싱되지 않음).
    fn insert_into_sorted_numeric(
        &self,
        values: &mut Vec<(f64, Vec<NodeId>)>,
        value: f64,
        node_id: NodeId,
    ) {
        // NaN 값은 인덱싱하지 않음 (비교 불가능)
        if value.is_nan() {
            return;
        }

        // value가 NaN이 아니므로 partial_cmp는 항상 Some을 반환
        match values.binary_search_by(|(v, _)| {
            // v가 NaN인 경우 (이전에 잘못 삽입된 경우) Greater로 처리하여 끝으로 밀어냄
            v.partial_cmp(&value).unwrap_or(std::cmp::Ordering::Greater)
        }) {
            Ok(index) => {
                // 동일한 값이 존재하면 노드 ID 추가
                if !values[index].1.contains(&node_id) {
                    values[index].1.push(node_id);
                }
            }
            Err(index) => {
                // 새로운 값이면 정렬된 위치에 삽입
                values.insert(index, (value, vec![node_id]));
            }
        }
    }

    /// 정렬된 문자열 값에 삽입
    fn insert_into_sorted_string(
        &self,
        values: &mut Vec<(String, Vec<NodeId>)>,
        value: String,
        node_id: NodeId,
    ) {
        match values.binary_search_by(|(v, _)| v.cmp(&value)) {
            Ok(index) => {
                // 동일한 값이 존재하면 노드 ID 추가
                if !values[index].1.contains(&node_id) {
                    values[index].1.push(node_id);
                }
            }
            Err(index) => {
                // 새로운 값이면 정렬된 위치에 삽입
                values.insert(index, (value, vec![node_id]));
            }
        }
    }
}

impl Default for GraphIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for IndexStats {
    fn clone(&self) -> Self {
        Self {
            node_index_hits: self.node_index_hits,
            edge_index_hits: self.edge_index_hits,
            composite_index_hits: self.composite_index_hits,
            range_query_hits: self.range_query_hits,
            total_queries: self.total_queries,
        }
    }
}
