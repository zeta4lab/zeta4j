// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 알고리즘 공통 타입 별칭
//!
//! `clippy::type_complexity` 경고를 해결하기 위한 타입 별칭 모듈입니다.
//! 복잡한 제네릭 타입들을 명확한 이름의 별칭으로 정의하여 코드 가독성을 향상시킵니다.

use crate::core::NodeId;
use std::collections::HashMap;

/// 무방향 인접 리스트
///
/// 각 노드에서 연결된 이웃 노드들의 목록을 저장합니다.
pub type AdjacencyList = HashMap<NodeId, Vec<NodeId>>;

/// 가중치 인접 리스트
///
/// 각 노드에서 연결된 이웃 노드와 엣지 가중치를 저장합니다.
pub type WeightedAdjacencyList = HashMap<NodeId, Vec<(NodeId, f64)>>;

/// DFS 발견 시간 맵
///
/// 깊이 우선 탐색에서 각 노드가 처음 발견된 시간을 저장합니다.
pub type DiscoveryTimeMap = HashMap<NodeId, u64>;

/// Low-link 값 맵
///
/// Tarjan 알고리즘에서 사용되는 low-link 값을 저장합니다.
/// 각 노드에서 도달 가능한 가장 낮은 발견 시간을 나타냅니다.
pub type LowLinkMap = HashMap<NodeId, u64>;

/// 선행자 맵
///
/// 최단 경로나 BFS에서 각 노드의 선행자(부모) 노드들을 저장합니다.
pub type PredecessorMap = HashMap<NodeId, Vec<NodeId>>;

/// 거리 맵
///
/// 소스 노드로부터 각 노드까지의 거리를 저장합니다.
pub type DistanceMap = HashMap<NodeId, f64>;

/// 시그마 맵 (Betweenness Centrality용)
///
/// 각 노드까지의 최단 경로 개수를 저장합니다.
pub type SigmaMap = HashMap<NodeId, f64>;

/// 델타 맵 (Betweenness Centrality용)
///
/// Betweenness centrality 계산에서 의존성 값을 저장합니다.
pub type DeltaMap = HashMap<NodeId, f64>;

/// 노드 점수 맵
///
/// PageRank, 중심성 등 알고리즘 결과에서 노드별 점수를 저장합니다.
pub type NodeScoreMap = HashMap<NodeId, f64>;

/// 컴포넌트 ID 맵
///
/// 연결 요소나 커뮤니티 탐지에서 각 노드의 소속 컴포넌트 ID를 저장합니다.
pub type ComponentMap = HashMap<NodeId, NodeId>;

/// 깊이 맵
///
/// BFS나 트리 탐색에서 각 노드의 깊이(레벨)를 저장합니다.
pub type DepthMap = HashMap<NodeId, usize>;

/// 부모 노드 맵
///
/// 트리 구조에서 각 노드의 부모 노드를 저장합니다.
/// `None`은 루트 노드를 나타냅니다.
pub type ParentMap = HashMap<NodeId, Option<NodeId>>;

/// 색상 맵 (그래프 컬러링용)
///
/// 그래프 컬러링에서 각 노드에 할당된 색상 번호를 저장합니다.
pub type ColorMap = HashMap<NodeId, usize>;

/// Core number 맵 (K-Core용)
///
/// K-Core 분해에서 각 노드의 core number를 저장합니다.
pub type CoreNumberMap = HashMap<NodeId, usize>;
