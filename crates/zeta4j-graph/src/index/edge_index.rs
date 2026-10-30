//! 엣지 전용 인덱싱 시스템

use crate::core::{EdgeId, MemoryReporter, NodeId, Value};
use crate::index::HashIndex;
use crate::index::range_index::{
    BTreeRangeIndex, fallback_float_range_search, fallback_integer_range_search,
};
use parking_lot::RwLock;
use std::collections::HashMap;

/// 엣지 인덱싱 시스템
#[derive(Debug)]
pub struct EdgeIndex {
    /// 관계 타입별 엣지 인덱스 (relation_type -> EdgeId set)
    relation_type_index: HashIndex<String, EdgeId>,

    /// 시작 노드별 엣지 인덱스 (from_node -> EdgeId set)
    from_node_index: HashIndex<NodeId, EdgeId>,

    /// 끝 노드별 엣지 인덱스 (to_node -> EdgeId set)
    to_node_index: HashIndex<NodeId, EdgeId>,

    /// 엣지 속성 키별 인덱스 (property_key -> EdgeId set)
    property_key_index: HashIndex<String, EdgeId>,

    /// 엣지 속성 값별 인덱스 (property_key -> (value -> EdgeId set))
    property_value_index: RwLock<HashMap<String, HashIndex<Value, EdgeId>>>,

    /// 복합 인덱스 (relation_type + property_key -> EdgeId set)
    composite_index: RwLock<HashMap<String, HashIndex<Value, EdgeId>>>,

    /// 노드 쌍 인덱스 ((from_node, to_node) -> EdgeId set)
    node_pair_index: HashIndex<(NodeId, NodeId), EdgeId>,

    /// 범위 검색용 B+Tree 인덱스 (property_key -> BTreeRangeIndex)
    range_index: RwLock<HashMap<String, BTreeRangeIndex<EdgeId>>>,
}

impl EdgeIndex {
    /// 새로운 엣지 인덱스 생성
    pub fn new() -> Self {
        Self {
            relation_type_index: HashIndex::new(),
            from_node_index: HashIndex::new(),
            to_node_index: HashIndex::new(),
            property_key_index: HashIndex::new(),
            property_value_index: RwLock::new(HashMap::new()),
            composite_index: RwLock::new(HashMap::new()),
            node_pair_index: HashIndex::new(),
            range_index: RwLock::new(HashMap::new()),
        }
    }

    /// 초기 용량을 지정한 엣지 인덱스 생성
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            relation_type_index: HashIndex::with_capacity(capacity),
            from_node_index: HashIndex::with_capacity(capacity),
            to_node_index: HashIndex::with_capacity(capacity),
            property_key_index: HashIndex::with_capacity(capacity),
            property_value_index: RwLock::new(HashMap::with_capacity(capacity)),
            composite_index: RwLock::new(HashMap::with_capacity(capacity)),
            node_pair_index: HashIndex::with_capacity(capacity),
            range_index: RwLock::new(HashMap::with_capacity(capacity)),
        }
    }

    /// 엣지의 기본 인덱싱
    pub fn add_edge(
        &self,
        edge_id: EdgeId,
        relation_type: &str,
        from_node: NodeId,
        to_node: NodeId,
    ) {
        // 관계 타입 인덱스에 추가
        self.relation_type_index
            .insert(relation_type.to_string(), edge_id);

        // 노드별 인덱스에 추가
        self.from_node_index.insert(from_node, edge_id);
        self.to_node_index.insert(to_node, edge_id);

        // 노드 쌍 인덱스에 추가
        self.node_pair_index.insert((from_node, to_node), edge_id);
    }

    /// 엣지의 기본 인덱스에서 제거
    pub fn remove_edge(
        &self,
        edge_id: EdgeId,
        relation_type: &str,
        from_node: NodeId,
        to_node: NodeId,
    ) {
        // 관계 타입 인덱스에서 제거
        self.relation_type_index
            .remove(&relation_type.to_string(), &edge_id);

        // 노드별 인덱스에서 제거
        self.from_node_index.remove(&from_node, &edge_id);
        self.to_node_index.remove(&to_node, &edge_id);

        // 노드 쌍 인덱스에서 제거
        self.node_pair_index.remove(&(from_node, to_node), &edge_id);
    }

    /// 엣지의 속성 인덱싱
    pub fn add_edge_property(&self, edge_id: EdgeId, key: &str, value: &Value) {
        // 속성 키 인덱스에 추가
        self.property_key_index.insert(key.to_string(), edge_id);

        // 속성 값 인덱스에 추가
        let mut property_value_index = self.property_value_index.write();
        property_value_index
            .entry(key.to_string())
            .or_default()
            .insert(value.clone(), edge_id);

        // 숫자 타입은 범위 인덱스에도 추가 (O(log n) 범위 검색 지원)
        match value {
            Value::Integer(v) => {
                let mut range_indices = self.range_index.write();
                range_indices
                    .entry(key.to_string())
                    .or_default()
                    .insert_integer(*v, edge_id);
            }
            Value::Float(v) => {
                let mut range_indices = self.range_index.write();
                range_indices
                    .entry(key.to_string())
                    .or_default()
                    .insert_float(*v, edge_id);
            }
            _ => {}
        }
    }

    /// 엣지의 속성 인덱스에서 제거
    pub fn remove_edge_property(&self, edge_id: EdgeId, key: &str, value: &Value) {
        // 속성 키 인덱스에서 제거
        self.property_key_index.remove(&key.to_string(), &edge_id);

        // 속성 값 인덱스에서 제거
        let mut property_value_index = self.property_value_index.write();
        if let Some(value_index) = property_value_index.get_mut(key) {
            value_index.remove(value, &edge_id);
            if value_index.is_empty() {
                property_value_index.remove(key);
            }
        }

        // 범위 인덱스에서도 제거
        match value {
            Value::Integer(v) => {
                let mut range_indices = self.range_index.write();
                if let Some(range_idx) = range_indices.get_mut(key) {
                    range_idx.remove_integer(*v, &edge_id);
                    if range_idx.is_empty() {
                        range_indices.remove(key);
                    }
                }
            }
            Value::Float(v) => {
                let mut range_indices = self.range_index.write();
                if let Some(range_idx) = range_indices.get_mut(key) {
                    range_idx.remove_float(*v, &edge_id);
                    if range_idx.is_empty() {
                        range_indices.remove(key);
                    }
                }
            }
            _ => {}
        }
    }

    /// 복합 인덱스 생성 (관계 타입 + 속성)
    pub fn create_composite_index(&self, relation_type: &str, property_key: &str) {
        let composite_key = format!("{}:{}", relation_type, property_key);
        let mut composite_index = self.composite_index.write();
        composite_index.insert(composite_key, HashIndex::new());
    }

    /// 복합 인덱스에 엣지 추가
    pub fn add_to_composite_index(
        &self,
        edge_id: EdgeId,
        relation_type: &str,
        property_key: &str,
        property_value: &Value,
    ) {
        let composite_key = format!("{}:{}", relation_type, property_key);
        let mut composite_index = self.composite_index.write();

        if let Some(index) = composite_index.get_mut(&composite_key) {
            index.insert(property_value.clone(), edge_id);
        }
    }

    /// 복합 인덱스에서 엣지 제거
    pub fn remove_from_composite_index(
        &self,
        edge_id: EdgeId,
        relation_type: &str,
        property_key: &str,
        property_value: &Value,
    ) {
        let composite_key = format!("{}:{}", relation_type, property_key);
        let mut composite_index = self.composite_index.write();

        if let Some(index) = composite_index.get_mut(&composite_key) {
            index.remove(property_value, &edge_id);
        }
    }

    /// 관계 타입으로 엣지들 검색
    pub fn find_edges_by_relation_type(&self, relation_type: &str) -> Vec<EdgeId> {
        // Neo4j 호환: 역순 반환 (LIFO - 마지막에 추가된 엣지가 먼저)
        let mut edges = self.relation_type_index.get(&relation_type.to_string());
        edges.reverse();
        edges
    }

    /// 시작 노드로 엣지들 검색 (나가는 엣지)
    pub fn find_edges_by_from_node(&self, from_node: NodeId) -> Vec<EdgeId> {
        // Neo4j 호환: 역순 반환 (LIFO - 마지막에 추가된 엣지가 먼저)
        let mut edges = self.from_node_index.get(&from_node);
        edges.reverse();
        edges
    }

    /// 끝 노드로 엣지들 검색 (들어오는 엣지)
    pub fn find_edges_by_to_node(&self, to_node: NodeId) -> Vec<EdgeId> {
        // Neo4j 호환: 역순 반환 (LIFO - 마지막에 추가된 엣지가 먼저)
        let mut edges = self.to_node_index.get(&to_node);
        edges.reverse();
        edges
    }

    /// 노드와 연결된 모든 엣지들 검색 (양방향)
    pub fn find_edges_by_node(&self, node_id: NodeId) -> Vec<EdgeId> {
        let mut edges = self.find_edges_by_from_node(node_id);
        edges.extend(self.find_edges_by_to_node(node_id));
        edges.sort();
        edges.dedup();
        edges
    }

    /// 노드 쌍으로 엣지들 검색
    pub fn find_edges_by_node_pair(&self, from_node: NodeId, to_node: NodeId) -> Vec<EdgeId> {
        // Neo4j 호환: 역순 반환 (LIFO - 마지막에 추가된 엣지가 먼저)
        let mut edges = self.node_pair_index.get(&(from_node, to_node));
        edges.reverse();
        edges
    }

    /// 양방향으로 노드 쌍 검색 (A->B 또는 B->A)
    pub fn find_edges_between_nodes(&self, node1: NodeId, node2: NodeId) -> Vec<EdgeId> {
        let mut edges = self.find_edges_by_node_pair(node1, node2);
        edges.extend(self.find_edges_by_node_pair(node2, node1));
        edges.sort();
        edges.dedup();
        edges
    }

    /// 특정 노드에서 특정 관계 타입으로 나가는 엣지들
    ///
    /// # 최적화 (v1.37.0)
    /// - 이전: O(n×m) - 두 Vec 생성 후 contains() 선형 검색
    /// - 현재: O(n) - HashIndex.contains()로 O(1) 룩업
    pub fn find_outgoing_edges_by_type(
        &self,
        from_node: NodeId,
        relation_type: &str,
    ) -> Vec<EdgeId> {
        let outgoing_edges = self.find_edges_by_from_node(from_node);
        let rel_type_key = relation_type.to_string();

        // O(1) 룩업으로 교집합 계산
        outgoing_edges
            .into_iter()
            .filter(|&edge_id| self.relation_type_index.contains(&rel_type_key, &edge_id))
            .collect()
    }

    /// 특정 노드로 특정 관계 타입으로 들어오는 엣지들
    ///
    /// # 최적화 (v1.37.0)
    /// - 이전: O(n×m) - 두 Vec 생성 후 contains() 선형 검색
    /// - 현재: O(n) - HashIndex.contains()로 O(1) 룩업
    pub fn find_incoming_edges_by_type(&self, to_node: NodeId, relation_type: &str) -> Vec<EdgeId> {
        let incoming_edges = self.find_edges_by_to_node(to_node);
        let rel_type_key = relation_type.to_string();

        // O(1) 룩업으로 교집합 계산
        incoming_edges
            .into_iter()
            .filter(|&edge_id| self.relation_type_index.contains(&rel_type_key, &edge_id))
            .collect()
    }

    /// 속성 키로 엣지들 검색
    pub fn find_edges_by_property_key(&self, key: &str) -> Vec<EdgeId> {
        self.property_key_index.get(&key.to_string())
    }

    /// 속성 키-값으로 엣지들 검색
    pub fn find_edges_by_property(&self, key: &str, value: &Value) -> Vec<EdgeId> {
        let property_value_index = self.property_value_index.read();
        property_value_index
            .get(key)
            .map_or(Vec::new(), |index| index.get(value))
    }

    /// 속성 값 범위로 엣지들 검색 (정수 값만)
    /// 시간 복잡도: O(log n + k) (k = 결과 수)
    pub fn find_edges_by_integer_range(&self, key: &str, min: i64, max: i64) -> Vec<EdgeId> {
        // BTreeRangeIndex 사용 (O(log n + k))
        let range_indices = self.range_index.read();
        if let Some(range_idx) = range_indices.get(key) {
            return range_idx.range_integer(min, max);
        }
        drop(range_indices);

        // 범위 인덱스가 없으면 기존 방식으로 폴백 (O(n))
        let property_value_index = self.property_value_index.read();
        property_value_index
            .get(key)
            .map(|index| fallback_integer_range_search(index, min, max))
            .unwrap_or_default()
    }

    /// 속성 값 범위로 엣지들 검색 (실수 값만)
    /// 시간 복잡도: O(log n + k) (k = 결과 수)
    pub fn find_edges_by_float_range(&self, key: &str, min: f64, max: f64) -> Vec<EdgeId> {
        // BTreeRangeIndex 사용 (O(log n + k))
        let range_indices = self.range_index.read();
        if let Some(range_idx) = range_indices.get(key) {
            return range_idx.range_float(min, max);
        }
        drop(range_indices);

        // 범위 인덱스가 없으면 기존 방식으로 폴백 (O(n))
        let property_value_index = self.property_value_index.read();
        property_value_index
            .get(key)
            .map(|index| fallback_float_range_search(index, min, max))
            .unwrap_or_default()
    }

    /// 복합 인덱스로 엣지들 검색
    pub fn find_edges_by_composite(
        &self,
        relation_type: &str,
        property_key: &str,
        property_value: &Value,
    ) -> Vec<EdgeId> {
        let composite_key = format!("{}:{}", relation_type, property_key);
        let composite_index = self.composite_index.read();

        composite_index
            .get(&composite_key)
            .map_or(Vec::new(), |index| index.get(property_value))
    }

    /// 다중 관계 타입 교집합 검색
    pub fn find_edges_by_relation_types_intersection(
        &self,
        relation_types: &[&str],
    ) -> Vec<EdgeId> {
        if relation_types.is_empty() {
            return Vec::new();
        }

        // 첫 번째 관계 타입의 엣지들을 기준으로 시작
        let mut result_edges: Vec<EdgeId> = self.find_edges_by_relation_type(relation_types[0]);

        // 나머지 관계 타입들과 교집합 계산
        for &relation_type in &relation_types[1..] {
            let type_edges = self.find_edges_by_relation_type(relation_type);
            result_edges.retain(|edge_id| type_edges.contains(edge_id));
        }

        result_edges
    }

    /// 다중 관계 타입 합집합 검색
    pub fn find_edges_by_relation_types_union(&self, relation_types: &[&str]) -> Vec<EdgeId> {
        let mut result_edges = Vec::new();
        let mut seen_edges = std::collections::HashSet::new();

        for &relation_type in relation_types {
            for edge_id in self.find_edges_by_relation_type(relation_type) {
                if seen_edges.insert(edge_id) {
                    result_edges.push(edge_id);
                }
            }
        }

        result_edges
    }

    /// 엣지의 모든 인덱스 항목 제거
    pub fn remove_edge_completely(&self, edge_id: EdgeId) {
        // 관계 타입 인덱스에서 제거
        let relation_types_to_remove: Vec<String> = self.relation_type_index.get_reverse(&edge_id);
        for relation_type in relation_types_to_remove {
            self.relation_type_index.remove(&relation_type, &edge_id);
        }

        // 노드 인덱스에서 제거
        let from_nodes_to_remove: Vec<NodeId> = self.from_node_index.get_reverse(&edge_id);
        for from_node in from_nodes_to_remove {
            self.from_node_index.remove(&from_node, &edge_id);
        }

        let to_nodes_to_remove: Vec<NodeId> = self.to_node_index.get_reverse(&edge_id);
        for to_node in to_nodes_to_remove {
            self.to_node_index.remove(&to_node, &edge_id);
        }

        // 노드 쌍 인덱스에서 제거
        let node_pairs_to_remove: Vec<(NodeId, NodeId)> =
            self.node_pair_index.get_reverse(&edge_id);
        for node_pair in node_pairs_to_remove {
            self.node_pair_index.remove(&node_pair, &edge_id);
        }

        // 속성 키 인덱스에서 제거
        let keys_to_remove: Vec<String> = self.property_key_index.get_reverse(&edge_id);
        for key in keys_to_remove {
            self.property_key_index.remove(&key, &edge_id);

            // 속성 값 인덱스에서도 제거
            let mut property_value_index = self.property_value_index.write();
            if let Some(value_index) = property_value_index.get_mut(&key) {
                let values_to_remove: Vec<Value> = value_index.get_reverse(&edge_id);
                for value in values_to_remove {
                    value_index.remove(&value, &edge_id);
                }
                if value_index.is_empty() {
                    property_value_index.remove(&key);
                }
            }
        }

        // 복합 인덱스에서 제거
        let mut composite_index = self.composite_index.write();
        for index in composite_index.values_mut() {
            let values_to_remove: Vec<Value> = index.get_reverse(&edge_id);
            for value in values_to_remove {
                index.remove(&value, &edge_id);
            }
        }
    }

    /// 인덱스 통계
    pub fn stats(&self) -> EdgeIndexStats {
        let property_value_index = self.property_value_index.read();
        let composite_index = self.composite_index.read();

        EdgeIndexStats {
            relation_type_count: self.relation_type_index.key_count(),
            property_key_count: self.property_key_index.key_count(),
            property_value_indexes: property_value_index.len(),
            composite_indexes: composite_index.len(),
            total_relation_mappings: self.relation_type_index.total_mappings(),
            total_property_mappings: self.property_key_index.total_mappings(),
            node_pair_mappings: self.node_pair_index.total_mappings(),
            memory_usage: self.memory_usage(),
        }
    }

    /// 메모리 사용량 추정
    pub fn memory_usage(&self) -> usize {
        let mut size = std::mem::size_of::<Self>();
        size += self.relation_type_index.memory_usage();
        size += self.from_node_index.memory_usage();
        size += self.to_node_index.memory_usage();
        size += self.property_key_index.memory_usage();
        size += self.node_pair_index.memory_usage();

        let property_value_index = self.property_value_index.read();
        for index in property_value_index.values() {
            size += index.memory_usage();
        }

        let composite_index = self.composite_index.read();
        for index in composite_index.values() {
            size += index.memory_usage();
        }

        size
    }

    /// 모든 인덱스 초기화
    pub fn clear(&self) {
        self.relation_type_index.clear();
        self.from_node_index.clear();
        self.to_node_index.clear();
        self.property_key_index.clear();
        self.node_pair_index.clear();

        let mut property_value_index = self.property_value_index.write();
        property_value_index.clear();

        let mut composite_index = self.composite_index.write();
        composite_index.clear();
    }

    /// 인덱스 압축
    pub fn compact(&self) {
        self.relation_type_index.compact();
        self.from_node_index.compact();
        self.to_node_index.compact();
        self.property_key_index.compact();
        self.node_pair_index.compact();

        let mut property_value_index = self.property_value_index.write();
        for index in property_value_index.values_mut() {
            index.compact();
        }

        let mut composite_index = self.composite_index.write();
        for index in composite_index.values_mut() {
            index.compact();
        }
    }
}

impl Default for EdgeIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// 엣지 인덱스 통계
#[derive(Debug, Clone)]
pub struct EdgeIndexStats {
    pub relation_type_count: usize,
    pub property_key_count: usize,
    pub property_value_indexes: usize,
    pub composite_indexes: usize,
    pub total_relation_mappings: usize,
    pub total_property_mappings: usize,
    pub node_pair_mappings: usize,
    pub memory_usage: usize,
}

impl MemoryReporter for EdgeIndexStats {
    fn memory_usage(&self) -> usize {
        self.memory_usage
    }
}
