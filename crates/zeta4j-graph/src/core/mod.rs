// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 핵심 그래프 데이터 구조

use parking_lot::RwLock;
use std::sync::Arc;

pub mod edge;
pub mod memory;
pub mod node;
pub mod property;
pub mod property_container;
pub mod serialization;
pub mod value;

pub use edge::{Edge, EdgeId};
pub use memory::{MemoryReporter, MemorySized};
pub use node::{Node, NodeId};
pub use property::Property;
pub use property_container::PropertyContainer;
pub use serialization::PostcardSerializable;
pub use value::{CypherDuration, Path, Value, VectorError, VectorValue};

// ============================================================================
// 타입 별칭: 참조 타입
// ============================================================================

/// 노드 불변 참조
pub type NodeRef<'a> = &'a Node;

/// 노드 가변 참조
pub type NodeRefMut<'a> = &'a mut Node;

/// 엣지 불변 참조
pub type EdgeRef<'a> = &'a Edge;

/// 엣지 가변 참조
pub type EdgeRefMut<'a> = &'a mut Edge;

// ============================================================================
// 타입 별칭: Arc 기반 공유 타입
// ============================================================================

/// 노드 공유 참조 (불변)
pub type NodeArc = Arc<Node>;

/// 노드 공유 참조 (가변, RwLock 래핑)
pub type NodeArcMut = Arc<RwLock<Node>>;

/// 엣지 공유 참조 (불변)
pub type EdgeArc = Arc<Edge>;

/// 엣지 공유 참조 (가변, RwLock 래핑)
pub type EdgeArcMut = Arc<RwLock<Edge>>;

// ============================================================================
// 타입 별칭: 컬렉션 타입
// ============================================================================

/// 노드 ID 목록
pub type NodeIdList = Vec<NodeId>;

/// 엣지 ID 목록
pub type EdgeIdList = Vec<EdgeId>;

/// 노드 목록
pub type NodeList = Vec<Node>;

/// 엣지 목록
pub type EdgeList = Vec<Edge>;

// ============================================================================
// 테스트
