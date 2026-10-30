//! BFS 기반 최단 경로 알고리즘
//!
//! 가중치가 없는 그래프에서 두 노드 간 최단 경로를 찾습니다.

use crate::core::{EdgeId, NodeId};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashSet, VecDeque};

/// BFS 최단 경로 결과
#[derive(Debug, Clone)]
pub struct ShortestPathResult {
    /// 경로상의 노드들 (시작 노드부터 끝 노드까지)
    pub nodes: Vec<NodeId>,
    /// 경로상의 엣지들
    pub edges: Vec<EdgeId>,
    /// 경로 길이 (엣지 수)
    pub distance: usize,
}

impl ShortestPathResult {
    /// 경로가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 경로 길이 반환
    pub fn len(&self) -> usize {
        self.distance
    }
}

/// BFS 기반 최단 경로 탐색기
pub struct BfsShortestPath<'a> {
    engine: &'a GraphEngine,
}

impl<'a> BfsShortestPath<'a> {
    /// 새로운 BFS 탐색기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self { engine }
    }

    /// 두 노드 간 최단 경로 찾기
    ///
    /// # Arguments
    /// * `start` - 시작 노드 ID
    /// * `end` - 끝 노드 ID
    /// * `max_depth` - 최대 탐색 깊이 (None이면 기본값 15)
    /// * `rel_types` - 탐색할 관계 타입들 (None이면 모든 타입)
    /// * `directed` - 방향성 고려 여부 (true면 from->to 방향만)
    ///
    /// # Returns
    /// * `Ok(Some(result))` - 경로를 찾은 경우
    /// * `Ok(None)` - 경로가 없는 경우
    pub async fn find(
        &self,
        start: NodeId,
        end: NodeId,
        max_depth: Option<usize>,
        rel_types: Option<&[String]>,
        directed: bool,
    ) -> Result<Option<ShortestPathResult>> {
        // 시작과 끝이 같으면 바로 반환
        if start == end {
            return Ok(Some(ShortestPathResult {
                nodes: vec![start],
                edges: vec![],
                distance: 0,
            }));
        }

        let max_depth = max_depth.unwrap_or(15);

        // BFS 초기화
        // (현재 노드, 경로상 노드들, 경로상 엣지들)
        let mut queue: VecDeque<(NodeId, Vec<NodeId>, Vec<EdgeId>)> = VecDeque::new();
        let mut visited: HashSet<NodeId> = HashSet::new();

        queue.push_back((start, vec![start], vec![]));
        visited.insert(start);

        while let Some((current, path, edges)) = queue.pop_front() {
            // 최대 깊이 초과 시 건너뛰기
            if edges.len() >= max_depth {
                continue;
            }

            // 이웃 탐색
            for (neighbor, edge_id) in self.get_neighbors(current, rel_types, directed).await? {
                if neighbor == end {
                    // 목적지 도착
                    let mut final_path = path.clone();
                    final_path.push(neighbor);
                    let mut final_edges = edges.clone();
                    final_edges.push(edge_id);
                    let distance = final_edges.len();

                    return Ok(Some(ShortestPathResult {
                        nodes: final_path,
                        edges: final_edges,
                        distance,
                    }));
                }

                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    let mut new_path = path.clone();
                    new_path.push(neighbor);
                    let mut new_edges = edges.clone();
                    new_edges.push(edge_id);
                    queue.push_back((neighbor, new_path, new_edges));
                }
            }
        }

        // 경로를 찾지 못함
        Ok(None)
    }

    /// 모든 최단 경로 찾기
    ///
    /// 같은 거리의 모든 최단 경로를 반환합니다.
    pub async fn find_all(
        &self,
        start: NodeId,
        end: NodeId,
        max_depth: Option<usize>,
        rel_types: Option<&[String]>,
        directed: bool,
    ) -> Result<Vec<ShortestPathResult>> {
        if start == end {
            return Ok(vec![ShortestPathResult {
                nodes: vec![start],
                edges: vec![],
                distance: 0,
            }]);
        }

        let max_depth = max_depth.unwrap_or(15);
        let mut results: Vec<ShortestPathResult> = Vec::new();
        let mut shortest_distance: Option<usize> = None;

        // BFS 초기화 - 방문한 노드를 거리와 함께 추적
        let mut queue: VecDeque<(NodeId, Vec<NodeId>, Vec<EdgeId>)> = VecDeque::new();
        let mut distance_to: std::collections::HashMap<NodeId, usize> =
            std::collections::HashMap::new();

        queue.push_back((start, vec![start], vec![]));
        distance_to.insert(start, 0);

        while let Some((current, path, edges)) = queue.pop_front() {
            let current_distance = edges.len();

            // 이미 더 짧은 경로를 찾았으면 건너뛰기
            if let Some(sd) = shortest_distance
                && current_distance >= sd
            {
                continue;
            }

            // 최대 깊이 초과
            if current_distance >= max_depth {
                continue;
            }

            for (neighbor, edge_id) in self.get_neighbors(current, rel_types, directed).await? {
                let new_distance = current_distance + 1;

                if neighbor == end {
                    // 목적지 도착
                    let mut final_path = path.clone();
                    final_path.push(neighbor);
                    let mut final_edges = edges.clone();
                    final_edges.push(edge_id);

                    match shortest_distance {
                        None => {
                            shortest_distance = Some(new_distance);
                            results.push(ShortestPathResult {
                                nodes: final_path,
                                edges: final_edges,
                                distance: new_distance,
                            });
                        }
                        Some(sd) if new_distance == sd => {
                            results.push(ShortestPathResult {
                                nodes: final_path,
                                edges: final_edges,
                                distance: new_distance,
                            });
                        }
                        _ => {}
                    }
                } else {
                    // 아직 방문하지 않았거나 같은 거리로 방문 가능한 경우
                    let should_visit = match distance_to.get(&neighbor) {
                        None => true,
                        Some(&d) => new_distance <= d,
                    };

                    if should_visit {
                        distance_to.insert(neighbor, new_distance);
                        let mut new_path = path.clone();
                        new_path.push(neighbor);
                        let mut new_edges = edges.clone();
                        new_edges.push(edge_id);
                        queue.push_back((neighbor, new_path, new_edges));
                    }
                }
            }
        }

        Ok(results)
    }

    /// 노드의 이웃들과 연결 엣지 가져오기
    async fn get_neighbors(
        &self,
        node: NodeId,
        rel_types: Option<&[String]>,
        directed: bool,
    ) -> Result<Vec<(NodeId, EdgeId)>> {
        let edge_ids = self.engine.find_edges_by_node(node)?;
        let mut neighbors = Vec::new();

        for edge_id in edge_ids {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(types) = rel_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                // 방향성에 따라 이웃 결정
                if directed {
                    // 방향성 있는 경우: from_node가 현재 노드일 때만
                    if edge.from_node() == node {
                        neighbors.push((edge.to_node(), edge_id));
                    }
                } else {
                    // 무방향: 양쪽 다 탐색
                    if let Some(other) = edge.other_node(node) {
                        neighbors.push((other, edge_id));
                    }
                }
            }
        }

        Ok(neighbors)
    }
}
