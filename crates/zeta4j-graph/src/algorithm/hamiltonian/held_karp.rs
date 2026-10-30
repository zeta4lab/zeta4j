//! Held-Karp 동적 프로그래밍 알고리즘
//!
//! 비트마스크 DP를 사용하여 최소 비용 Hamiltonian Path/Circuit을 찾습니다.
//! TSP(Traveling Salesman Problem) 해결에도 사용됩니다.
//!
//! 시간복잡도: O(n² * 2^n)
//! 공간복잡도: O(n * 2^n)

use super::HamiltonianConfig;
use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::{GraphError, Result};
use std::collections::HashMap;

const INF: f64 = f64::MAX / 2.0;

/// Held-Karp DP 알고리즘
pub struct HeldKarp<'a> {
    engine: &'a GraphEngine,
    config: HamiltonianConfig,
}

/// 가중치 Hamiltonian 결과
#[derive(Debug, Clone)]
pub struct WeightedHamiltonianResult {
    /// 경로
    pub path: Vec<NodeId>,
    /// 총 비용
    pub total_cost: f64,
    /// 회로 여부
    pub is_circuit: bool,
    /// 발견 여부
    pub found: bool,
}

impl WeightedHamiltonianResult {
    /// 빈 결과
    pub fn empty() -> Self {
        Self {
            path: Vec::new(),
            total_cost: 0.0,
            is_circuit: false,
            found: true,
        }
    }

    /// 발견 실패
    pub fn not_found() -> Self {
        Self {
            path: Vec::new(),
            total_cost: 0.0,
            is_circuit: false,
            found: false,
        }
    }
}

impl<'a> HeldKarp<'a> {
    /// 새로운 HeldKarp 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: HamiltonianConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: HamiltonianConfig) -> Self {
        Self { engine, config }
    }

    /// 최소 비용 Hamiltonian Path 찾기
    #[expect(clippy::needless_range_loop)]
    pub async fn min_path(&self) -> Result<Option<WeightedHamiltonianResult>> {
        let (nodes, dist) = self.build_distance_matrix().await?;
        let n = nodes.len();

        if n == 0 {
            return Ok(Some(WeightedHamiltonianResult::empty()));
        }

        if n == 1 {
            return Ok(Some(WeightedHamiltonianResult {
                path: nodes,
                total_cost: 0.0,
                is_circuit: false,
                found: true,
            }));
        }

        // 노드 수 제한 (2^n 메모리 필요)
        if n > self.config.max_nodes.min(20) {
            return Err(GraphError::Validation {
                message: format!(
                    "Held-Karp requires n <= 20 for memory constraints. Got {}",
                    n
                ),
            });
        }

        // dp[mask][i] = mask 집합의 노드들을 방문하고 i에서 끝나는 최소 비용
        // parent[mask][i] = 최적 경로에서 i 이전 노드
        let mut dp = vec![vec![INF; n]; 1 << n];
        let mut parent = vec![vec![usize::MAX; n]; 1 << n];

        // 모든 시작점에서 시작
        for i in 0..n {
            dp[1 << i][i] = 0.0;
        }

        // DP 진행
        for mask in 1..(1 << n) {
            for last in 0..n {
                if mask & (1 << last) == 0 || dp[mask][last] >= INF {
                    continue;
                }

                for next in 0..n {
                    if mask & (1 << next) != 0 {
                        continue;
                    }

                    let edge_cost = dist[last][next];
                    if edge_cost >= INF {
                        continue;
                    }

                    let new_mask = mask | (1 << next);
                    let new_cost = dp[mask][last] + edge_cost;

                    if new_cost < dp[new_mask][next] {
                        dp[new_mask][next] = new_cost;
                        parent[new_mask][next] = last;
                    }
                }
            }
        }

        // 최종 마스크 (모든 노드 방문)
        let full_mask = (1 << n) - 1;

        // 최소 비용 끝점 찾기
        let mut min_cost = INF;
        let mut end_node = usize::MAX;

        for i in 0..n {
            if dp[full_mask][i] < min_cost {
                min_cost = dp[full_mask][i];
                end_node = i;
            }
        }

        if min_cost >= INF {
            return Ok(None);
        }

        // 경로 역추적
        let mut path = Vec::new();
        let mut mask = full_mask;
        let mut current = end_node;

        while current != usize::MAX {
            path.push(nodes[current]);
            let prev = parent[mask][current];
            mask ^= 1 << current;
            current = prev;
        }

        path.reverse();

        Ok(Some(WeightedHamiltonianResult {
            path,
            total_cost: min_cost,
            is_circuit: false,
            found: true,
        }))
    }

    /// 최소 비용 Hamiltonian Circuit (TSP) 찾기
    pub async fn min_circuit(&self) -> Result<Option<WeightedHamiltonianResult>> {
        let (nodes, dist) = self.build_distance_matrix().await?;
        let n = nodes.len();

        if n == 0 {
            return Ok(Some(WeightedHamiltonianResult::empty()));
        }

        if n == 1 {
            return Ok(None); // 단일 노드는 circuit 불가
        }

        if n == 2 {
            // 두 노드가 서로 연결되어야 함
            if dist[0][1] < INF && dist[1][0] < INF {
                return Ok(Some(WeightedHamiltonianResult {
                    path: vec![nodes[0], nodes[1], nodes[0]],
                    total_cost: dist[0][1] + dist[1][0],
                    is_circuit: true,
                    found: true,
                }));
            }
            return Ok(None);
        }

        // 노드 수 제한
        if n > self.config.max_nodes.min(20) {
            return Err(GraphError::Validation {
                message: format!("Held-Karp requires n <= 20. Got {}", n),
            });
        }

        // TSP: 시작점 0에서 시작하고 0으로 돌아옴
        let start = 0;

        let mut dp = vec![vec![INF; n]; 1 << n];
        let mut parent = vec![vec![usize::MAX; n]; 1 << n];

        dp[1 << start][start] = 0.0;

        for mask in 1..(1 << n) {
            if mask & (1 << start) == 0 {
                continue;
            }

            for last in 0..n {
                if mask & (1 << last) == 0 || dp[mask][last] >= INF {
                    continue;
                }

                for next in 0..n {
                    if mask & (1 << next) != 0 {
                        continue;
                    }

                    let edge_cost = dist[last][next];
                    if edge_cost >= INF {
                        continue;
                    }

                    let new_mask = mask | (1 << next);
                    let new_cost = dp[mask][last] + edge_cost;

                    if new_cost < dp[new_mask][next] {
                        dp[new_mask][next] = new_cost;
                        parent[new_mask][next] = last;
                    }
                }
            }
        }

        let full_mask = (1 << n) - 1;

        // 시작점으로 돌아가는 최소 비용
        let mut min_cost = INF;
        let mut end_node = usize::MAX;

        for i in 0..n {
            if i == start {
                continue;
            }
            let return_cost = dp[full_mask][i] + dist[i][start];
            if return_cost < min_cost {
                min_cost = return_cost;
                end_node = i;
            }
        }

        if min_cost >= INF {
            return Ok(None);
        }

        // 경로 역추적
        let mut path = Vec::new();
        let mut mask = full_mask;
        let mut current = end_node;

        while current != usize::MAX {
            path.push(nodes[current]);
            let prev = parent[mask][current];
            mask ^= 1 << current;
            current = prev;
        }

        path.reverse();
        path.push(nodes[start]); // 시작점으로 돌아옴

        Ok(Some(WeightedHamiltonianResult {
            path,
            total_cost: min_cost,
            is_circuit: true,
            found: true,
        }))
    }

    /// 거리 행렬 구축
    #[expect(clippy::needless_range_loop)]
    async fn build_distance_matrix(&self) -> Result<(Vec<NodeId>, Vec<Vec<f64>>)> {
        let mut nodes: Vec<NodeId> = Vec::new();
        let mut node_to_idx: HashMap<NodeId, usize> = HashMap::new();

        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                node_to_idx.insert(node_id, nodes.len());
                nodes.push(node_id);
            }
        }

        let n = nodes.len();
        let mut dist = vec![vec![INF; n]; n];

        // 자기 자신으로의 거리는 0
        for i in 0..n {
            dist[i][i] = 0.0;
        }

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from == to {
                    continue;
                }

                if let (Some(&i), Some(&j)) = (node_to_idx.get(&from), node_to_idx.get(&to)) {
                    // 가중치 속성 가져오기
                    let weight = if let Some(ref prop) = self.config.weight_property {
                        edge.properties()
                            .get(prop)
                            .and_then(|v| match v {
                                crate::Value::Integer(i) => Some(*i as f64),
                                crate::Value::Float(f) => Some(*f),
                                _ => None,
                            })
                            .unwrap_or(1.0)
                    } else {
                        1.0
                    };

                    // 무방향 그래프
                    dist[i][j] = dist[i][j].min(weight);
                    dist[j][i] = dist[j][i].min(weight);
                }
            }
        }

        Ok((nodes, dist))
    }
}

/// 최소 비용 Hamiltonian Path (TSP 변형)
pub async fn min_hamiltonian_path(
    engine: &GraphEngine,
) -> Result<Option<WeightedHamiltonianResult>> {
    HeldKarp::new(engine).min_path().await
}

/// 최소 비용 Hamiltonian Circuit (TSP)
pub async fn min_hamiltonian_circuit(
    engine: &GraphEngine,
) -> Result<Option<WeightedHamiltonianResult>> {
    HeldKarp::new(engine).min_circuit().await
}
