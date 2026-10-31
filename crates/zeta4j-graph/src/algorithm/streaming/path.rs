// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 경로 알고리즘 스트리밍 구현
//!
//! 대규모 그래프에서 메모리 효율적인 경로 계산

use super::config::{AlgorithmPhase, StreamingConfig, StreamingStats};
use crate::core::{NodeId, PropertyContainer, Value};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, VecDeque};

/// 거리 쌍 (소스, 타겟, 거리)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistancePair {
    /// 시작 노드
    pub source: NodeId,
    /// 도착 노드
    pub target: NodeId,
    /// 거리
    pub distance: f64,
}

impl DistancePair {
    /// 새 거리 쌍 생성
    pub fn new(source: NodeId, target: NodeId, distance: f64) -> Self {
        Self {
            source,
            target,
            distance,
        }
    }
}

/// 청크 기반 Floyd-Warshall 설정
#[derive(Debug, Clone)]
pub struct ChunkedFloydWarshallConfig {
    /// 가중치 프로퍼티 이름
    pub weight_property: String,
    /// 기본 가중치
    pub default_weight: f64,
    /// 무한대 값
    pub infinity: f64,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
    /// 소스 노드 필터 (None이면 모든 노드)
    pub source_nodes: Option<Vec<NodeId>>,
    /// 최대 거리 (이보다 긴 경로 무시)
    pub max_distance: Option<f64>,
}

impl Default for ChunkedFloydWarshallConfig {
    fn default() -> Self {
        Self {
            weight_property: "weight".to_string(),
            default_weight: 1.0,
            infinity: f64::INFINITY,
            relationship_types: None,
            source_nodes: None,
            max_distance: None,
        }
    }
}

impl ChunkedFloydWarshallConfig {
    /// 가중치 프로퍼티 설정
    pub fn with_weight_property(mut self, property: &str) -> Self {
        self.weight_property = property.to_string();
        self
    }

    /// 소스 노드 필터 설정
    pub fn with_source_nodes(mut self, nodes: Vec<NodeId>) -> Self {
        self.source_nodes = Some(nodes);
        self
    }

    /// 최대 거리 설정
    pub fn with_max_distance(mut self, max: f64) -> Self {
        self.max_distance = Some(max);
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// 청크 기반 거리 계산 결과
#[derive(Debug)]
pub struct ChunkedDistanceResult {
    /// 스트리밍 통계
    pub stats: StreamingStats,
    /// 처리된 소스 노드 수
    pub sources_processed: usize,
    /// 계산된 거리 쌍 수
    pub distances_computed: usize,
}

/// 부분 경로 반복자
///
/// 메모리 효율적으로 거리 쌍을 하나씩 반환
pub struct PartialPathIterator<'a> {
    #[expect(dead_code)] // 향후 동적 그래프 접근에 사용
    engine: &'a GraphEngine,
    config: ChunkedFloydWarshallConfig,
    streaming_config: StreamingConfig,

    // 상태
    #[expect(dead_code)] // 향후 전체 노드 순회에 사용
    all_nodes: Vec<NodeId>,
    source_nodes: VecDeque<NodeId>,
    current_source: Option<NodeId>,
    current_distances: VecDeque<DistancePair>,

    // 그래프 구조 (엣지 인접 리스트)
    adjacency: HashMap<NodeId, Vec<(NodeId, f64)>>,

    // 통계
    sources_processed: usize,
    distances_emitted: usize,
}

impl<'a> PartialPathIterator<'a> {
    /// 새 반복자 생성
    pub async fn new(
        engine: &'a GraphEngine,
        config: ChunkedFloydWarshallConfig,
        streaming_config: StreamingConfig,
    ) -> Result<Self> {
        // 노드 수집
        let mut all_nodes = Vec::new();
        let node_count = engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if engine.get_node(node_id).await?.is_some() {
                all_nodes.push(node_id);
            }
        }

        // 소스 노드 결정
        let source_nodes: VecDeque<NodeId> = match &config.source_nodes {
            Some(nodes) => nodes.iter().cloned().collect(),
            None => all_nodes.iter().cloned().collect(),
        };

        // 그래프 구조 빌드
        let adjacency = Self::build_adjacency(engine, &config).await?;

        Ok(Self {
            engine,
            config,
            streaming_config,
            all_nodes,
            source_nodes,
            current_source: None,
            current_distances: VecDeque::new(),
            adjacency,
            sources_processed: 0,
            distances_emitted: 0,
        })
    }

    /// 인접 리스트 빌드
    async fn build_adjacency(
        engine: &GraphEngine,
        config: &ChunkedFloydWarshallConfig,
    ) -> Result<HashMap<NodeId, Vec<(NodeId, f64)>>> {
        let mut adjacency: HashMap<NodeId, Vec<(NodeId, f64)>> = HashMap::new();

        let edge_count = engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // 가중치 추출
                let weight = edge
                    .get_property(&config.weight_property)
                    .and_then(|v| match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .unwrap_or(config.default_weight);

                adjacency.entry(from).or_default().push((to, weight));
            }
        }

        Ok(adjacency)
    }

    /// 단일 소스에서 Dijkstra 실행
    fn dijkstra_from_source(&self, source: NodeId) -> HashMap<NodeId, f64> {
        use std::cmp::Ordering;
        use std::collections::BinaryHeap;

        #[derive(Clone, PartialEq)]
        struct State {
            cost: f64,
            node: NodeId,
        }

        impl Eq for State {}

        impl Ord for State {
            fn cmp(&self, other: &Self) -> Ordering {
                other
                    .cost
                    .partial_cmp(&self.cost)
                    .unwrap_or(Ordering::Equal)
            }
        }

        impl PartialOrd for State {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        let mut distances: HashMap<NodeId, f64> = HashMap::new();
        let mut heap = BinaryHeap::new();

        distances.insert(source, 0.0);
        heap.push(State {
            cost: 0.0,
            node: source,
        });

        while let Some(State { cost, node }) = heap.pop() {
            if let Some(&best) = distances.get(&node)
                && cost > best
            {
                continue;
            }

            if let Some(neighbors) = self.adjacency.get(&node) {
                for &(next, weight) in neighbors {
                    let next_cost = cost + weight;

                    // 최대 거리 체크
                    if let Some(max) = self.config.max_distance
                        && next_cost > max
                    {
                        continue;
                    }

                    let is_better = distances.get(&next).is_none_or(|&d| next_cost < d);

                    if is_better {
                        distances.insert(next, next_cost);
                        heap.push(State {
                            cost: next_cost,
                            node: next,
                        });
                    }
                }
            }
        }

        distances
    }

    /// 다음 소스 노드에서 거리 계산
    fn compute_next_batch(&mut self) {
        while self.current_distances.is_empty() {
            if let Some(source) = self.source_nodes.pop_front() {
                self.current_source = Some(source);

                // Dijkstra로 이 소스에서 모든 도달 가능한 노드까지의 거리 계산
                let distances = self.dijkstra_from_source(source);

                // 결과를 큐에 추가
                for (target, distance) in distances {
                    if source != target && distance.is_finite() {
                        self.current_distances
                            .push_back(DistancePair::new(source, target, distance));
                    }
                }

                self.sources_processed += 1;

                // 진행 상황 보고
                let total_sources = self.sources_processed + self.source_nodes.len();
                let progress = if total_sources > 0 {
                    self.sources_processed as f64 / total_sources as f64
                } else {
                    1.0
                };
                self.streaming_config.report_progress(
                    progress,
                    AlgorithmPhase::Computing,
                    self.sources_processed,
                );

                // 조기 종료 체크
                if self.streaming_config.should_stop() {
                    self.source_nodes.clear();
                    break;
                }
            } else {
                break;
            }
        }
    }

    /// 처리 통계 반환
    pub fn stats(&self) -> StreamingStats {
        StreamingStats {
            nodes_processed: self.sources_processed,
            edges_processed: 0,
            chunks_used: self.sources_processed,
            elapsed_ms: 0,
            peak_memory_bytes: 0,
        }
    }
}

impl<'a> Iterator for PartialPathIterator<'a> {
    type Item = DistancePair;

    fn next(&mut self) -> Option<Self::Item> {
        // 현재 버퍼에 결과가 없으면 다음 배치 계산
        if self.current_distances.is_empty() {
            self.compute_next_batch();
        }

        // 결과 반환
        if let Some(pair) = self.current_distances.pop_front() {
            self.distances_emitted += 1;
            Some(pair)
        } else {
            None
        }
    }
}

/// 청크 기반 Floyd-Warshall (SSSP 기반)
///
/// 전통적인 O(V³) Floyd-Warshall 대신 각 소스에서 Dijkstra를 실행하여
/// 메모리 사용량을 O(V) 수준으로 유지합니다.
pub struct ChunkedFloydWarshall<'a> {
    engine: &'a GraphEngine,
    config: ChunkedFloydWarshallConfig,
    streaming_config: StreamingConfig,
}

impl<'a> ChunkedFloydWarshall<'a> {
    /// 새 청크 기반 Floyd-Warshall 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ChunkedFloydWarshallConfig::default(),
            streaming_config: StreamingConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(
        engine: &'a GraphEngine,
        config: ChunkedFloydWarshallConfig,
        streaming_config: StreamingConfig,
    ) -> Self {
        Self {
            engine,
            config,
            streaming_config,
        }
    }

    /// 스트리밍 반복자 반환
    pub async fn compute_streaming(self) -> Result<PartialPathIterator<'a>> {
        PartialPathIterator::new(self.engine, self.config, self.streaming_config).await
    }

    /// 전체 결과 수집 (메모리에 로드)
    pub async fn compute_all(
        self,
    ) -> Result<(HashMap<(NodeId, NodeId), f64>, ChunkedDistanceResult)> {
        let start = std::time::Instant::now();
        let mut distances = HashMap::new();

        let mut iter = self.compute_streaming().await?;
        for pair in &mut iter {
            distances.insert((pair.source, pair.target), pair.distance);
        }

        let stats = iter.stats();
        let result = ChunkedDistanceResult {
            stats: StreamingStats {
                elapsed_ms: start.elapsed().as_millis() as u64,
                ..stats
            },
            sources_processed: iter.sources_processed,
            distances_computed: distances.len(),
        };

        Ok((distances, result))
    }
}
