//! 그래프 엣지 구조

use crate::core::serialization::PostcardSerializable;
use crate::core::{MemorySized, NodeId, Property, PropertyContainer};
use serde::{Deserialize, Serialize};

/// 엣지 ID 타입
pub type EdgeId = u64;

/// 그래프 엣지 (관계)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// 엣지 고유 ID
    id: EdgeId,

    /// 관계 타입 (레이블)
    relation_type: String,

    /// 시작 노드 ID
    from_node: NodeId,

    /// 끝 노드 ID
    to_node: NodeId,

    /// 엣지 속성
    properties: Property,
}

impl Edge {
    /// 새로운 엣지 생성
    pub fn new<S: Into<String>>(
        id: EdgeId,
        relation_type: S,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Self {
        Self {
            id,
            relation_type: relation_type.into(),
            from_node,
            to_node,
            properties: Property::new(),
        }
    }

    /// 엣지 ID 조회
    pub fn id(&self) -> EdgeId {
        self.id
    }

    /// 관계 타입 조회
    pub fn relation_type(&self) -> &str {
        &self.relation_type
    }

    /// 관계 타입 설정
    pub fn set_relation_type<S: Into<String>>(&mut self, relation_type: S) {
        self.relation_type = relation_type.into();
    }

    /// 시작 노드 ID 조회
    pub fn from_node(&self) -> NodeId {
        self.from_node
    }

    /// 끝 노드 ID 조회  
    pub fn to_node(&self) -> NodeId {
        self.to_node
    }

    /// 시작 노드 ID 설정
    pub fn set_from_node(&mut self, from_node: NodeId) {
        self.from_node = from_node;
    }

    /// 끝 노드 ID 설정
    pub fn set_to_node(&mut self, to_node: NodeId) {
        self.to_node = to_node;
    }

    /// 엣지가 특정 노드와 연결되어 있는지 확인
    pub fn connects_to(&self, node_id: NodeId) -> bool {
        self.from_node == node_id || self.to_node == node_id
    }

    /// 엣지가 두 노드 사이를 연결하는지 확인
    pub fn connects_nodes(&self, node1: NodeId, node2: NodeId) -> bool {
        (self.from_node == node1 && self.to_node == node2)
            || (self.from_node == node2 && self.to_node == node1)
    }

    /// 엣지가 방향성을 고려해서 두 노드를 연결하는지 확인
    pub fn connects_directed(&self, from: NodeId, to: NodeId) -> bool {
        self.from_node == from && self.to_node == to
    }

    /// 특정 노드에서 상대방 노드 ID 조회
    pub fn other_node(&self, node_id: NodeId) -> Option<NodeId> {
        if self.from_node == node_id {
            Some(self.to_node)
        } else if self.to_node == node_id {
            Some(self.from_node)
        } else {
            None
        }
    }

    /// 엣지 방향 반전
    pub fn reverse(&mut self) {
        std::mem::swap(&mut self.from_node, &mut self.to_node);
    }

    /// 반전된 엣지 복사본 생성
    pub fn reversed(&self) -> Self {
        let mut reversed = self.clone();
        reversed.reverse();
        reversed
    }

    // Property 메서드들은 PropertyContainer 트레이트로 이동됨
    // set_property, get_property, remove_property, has_property,
    // properties, properties_mut, property_count

    /// 관계 타입이 일치하는지 확인
    pub fn matches_type(&self, relation_type: &str) -> bool {
        self.relation_type == relation_type
    }

    /// 엣지의 메모리 사용량 추정 (바이트)
    pub fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.relation_type.len())
            .saturating_add(self.properties.memory_size())
    }
}

/// Edge에 대한 Postcard 직렬화 구현
impl PostcardSerializable for Edge {}

impl MemorySized for Edge {
    fn memory_size(&self) -> usize {
        Self::memory_size(self)
    }
}

/// Edge에 대한 PropertyContainer 구현
impl PropertyContainer for Edge {
    fn properties(&self) -> &Property {
        &self.properties
    }

    fn properties_mut(&mut self) -> &mut Property {
        &mut self.properties
    }
}
