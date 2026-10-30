//! Hungarian Algorithm (Kuhn-Munkres) 가중치 이분 매칭
//!
//! 가중치가 있는 이분 그래프에서 최소/최대 비용 매칭을 찾습니다.
//!
//! 시간복잡도: O(V³)

use super::bipartite::{BipartiteChecker, BipartiteConfig};
use crate::core::{NodeId, PropertyContainer, Value};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Hungarian 알고리즘 설정
#[derive(Debug, Clone)]
pub struct HungarianConfig {
    /// 가중치 프로퍼티 이름
    pub weight_property: String,
    /// 기본 가중치 (프로퍼티 없을 때)
    pub default_weight: f64,
    /// 최대화 여부 (true: 최대 가중치, false: 최소 가중치)
    pub maximize: bool,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl Default for HungarianConfig {
    fn default() -> Self {
        Self {
            weight_property: "weight".to_string(),
            default_weight: 1.0,
            maximize: false, // 기본: 최소 비용
            relationship_types: None,
        }
    }
}

impl HungarianConfig {
    /// 가중치 프로퍼티 설정
    pub fn with_weight_property(mut self, property: &str) -> Self {
        self.weight_property = property.to_string();
        self
    }

    /// 기본 가중치 설정
    pub fn with_default_weight(mut self, weight: f64) -> Self {
        self.default_weight = weight;
        self
    }

    /// 최대화 설정
    pub fn with_maximize(mut self, maximize: bool) -> Self {
        self.maximize = maximize;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Hungarian 알고리즘 결과
#[derive(Debug, Clone)]
pub struct HungarianResult {
    /// 매칭된 쌍들 (left, right)
    pub matches: Vec<(NodeId, NodeId)>,
    /// 총 가중치
    pub total_weight: f64,
    /// 할당 맵 (left -> right)
    pub assignment: HashMap<NodeId, NodeId>,
    /// 매칭 크기
    pub matching_size: usize,
    /// 최대화 모드 여부
    pub maximized: bool,
}

impl HungarianResult {
    /// 빈 결과
    pub fn empty(maximized: bool) -> Self {
        Self {
            matches: Vec::new(),
            total_weight: 0.0,
            assignment: HashMap::new(),
            matching_size: 0,
            maximized,
        }
    }

    /// 특정 노드의 매칭 상대 찾기
    pub fn get_assignment(&self, node_id: NodeId) -> Option<NodeId> {
        self.assignment.get(&node_id).copied()
    }
}

/// Hungarian Algorithm 구현
pub struct Hungarian<'a> {
    engine: &'a GraphEngine,
    config: HungarianConfig,
}

impl<'a> Hungarian<'a> {
    /// 새로운 Hungarian 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: HungarianConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: HungarianConfig) -> Self {
        Self { engine, config }
    }

    /// 가중치 매칭 찾기
    pub async fn find_matching(&self) -> Result<HungarianResult> {
        // 이분 그래프 확인
        let bipartite_config = BipartiteConfig::default();
        let checker = BipartiteChecker::with_config(self.engine, bipartite_config);
        let bipartite_result = checker.check().await?;

        if !bipartite_result.is_bipartite {
            return Ok(HungarianResult::empty(self.config.maximize));
        }

        let left_partition = &bipartite_result.left_partition;
        let right_partition = &bipartite_result.right_partition;

        if left_partition.is_empty() || right_partition.is_empty() {
            return Ok(HungarianResult::empty(self.config.maximize));
        }

        // 비용 행렬 구축
        let cost_matrix = self
            .build_cost_matrix(left_partition, right_partition)
            .await?;
        let n = left_partition.len().max(right_partition.len());

        if n == 0 {
            return Ok(HungarianResult::empty(self.config.maximize));
        }

        // 정사각 행렬로 패딩 (필요시)
        let mut matrix = vec![vec![0.0_f64; n]; n];
        let inf_cost = if self.config.maximize {
            f64::MIN
        } else {
            f64::MAX
        };

        for i in 0..n {
            for j in 0..n {
                if i < left_partition.len() && j < right_partition.len() {
                    let left = left_partition[i];
                    let right = right_partition[j];
                    matrix[i][j] = *cost_matrix.get(&(left, right)).unwrap_or(&inf_cost);
                } else {
                    matrix[i][j] = 0.0;
                }
            }
        }

        // 최대화 문제는 부호를 뒤집어 최소화로 변환
        if self.config.maximize {
            let max_val = matrix
                .iter()
                .flat_map(|row| row.iter())
                .filter(|&&v| v != f64::MIN)
                .cloned()
                .fold(f64::MIN, f64::max);

            for row in &mut matrix {
                for val in row {
                    if *val != f64::MIN {
                        *val = max_val - *val;
                    } else {
                        *val = max_val * 2.0; // 큰 값으로 설정
                    }
                }
            }
        }

        // Hungarian Algorithm 실행
        let assignment = self.hungarian_algorithm(&matrix, n);

        // 결과 구성
        let mut matches: Vec<(NodeId, NodeId)> = Vec::new();
        let mut assignment_map: HashMap<NodeId, NodeId> = HashMap::new();
        let mut total_weight = 0.0;

        for (i, &j) in assignment.iter().enumerate() {
            if i < left_partition.len() && j < right_partition.len() {
                let left = left_partition[i];
                let right = right_partition[j];

                if let Some(&weight) = cost_matrix.get(&(left, right))
                    && weight != f64::MAX
                    && weight != f64::MIN
                {
                    matches.push((left, right));
                    assignment_map.insert(left, right);
                    total_weight += weight;
                }
            }
        }

        matches.sort();

        Ok(HungarianResult {
            matching_size: matches.len(),
            matches,
            total_weight,
            assignment: assignment_map,
            maximized: self.config.maximize,
        })
    }

    /// Hungarian Algorithm 핵심 구현
    fn hungarian_algorithm(&self, cost: &[Vec<f64>], n: usize) -> Vec<usize> {
        // u, v: 잠재값 (potential)
        let mut u = vec![0.0_f64; n + 1];
        let mut v = vec![0.0_f64; n + 1];

        // p: 열 j가 어떤 행에 할당되었는지 (0이면 미할당)
        let mut p = vec![0_usize; n + 1];

        // way: 증가 경로 추적용
        let mut way = vec![0_usize; n + 1];

        for i in 1..=n {
            // 행 i 처리
            p[0] = i;
            let mut j0 = 0_usize; // 현재 열

            let mut minv = vec![f64::MAX; n + 1];
            let mut used = vec![false; n + 1];

            loop {
                used[j0] = true;
                let i0 = p[j0];
                let mut delta = f64::MAX;
                let mut j1 = 0_usize;

                for j in 1..=n {
                    if !used[j] {
                        let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                        if cur < minv[j] {
                            minv[j] = cur;
                            way[j] = j0;
                        }
                        if minv[j] < delta {
                            delta = minv[j];
                            j1 = j;
                        }
                    }
                }

                // 잠재값 업데이트
                for j in 0..=n {
                    if used[j] {
                        u[p[j]] += delta;
                        v[j] -= delta;
                    } else {
                        minv[j] -= delta;
                    }
                }

                j0 = j1;

                if p[j0] == 0 {
                    break;
                }
            }

            // 경로 따라 할당 업데이트
            loop {
                let j1 = way[j0];
                p[j0] = p[j1];
                j0 = j1;
                if j0 == 0 {
                    break;
                }
            }
        }

        // 결과: row i -> column (p의 역)
        let mut ans = vec![0_usize; n];
        for j in 1..=n {
            if p[j] > 0 && p[j] <= n {
                ans[p[j] - 1] = j - 1;
            }
        }

        ans
    }

    /// 비용 행렬 구축
    async fn build_cost_matrix(
        &self,
        left_partition: &[NodeId],
        right_partition: &[NodeId],
    ) -> Result<HashMap<(NodeId, NodeId), f64>> {
        let mut cost_matrix: HashMap<(NodeId, NodeId), f64> = HashMap::new();

        let left_set: std::collections::HashSet<NodeId> = left_partition.iter().copied().collect();
        let right_set: std::collections::HashSet<NodeId> =
            right_partition.iter().copied().collect();

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // 가중치 추출
                let weight = edge
                    .get_property(&self.config.weight_property)
                    .and_then(|v| match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .unwrap_or(self.config.default_weight);

                // 양방향 확인
                if left_set.contains(&from) && right_set.contains(&to) {
                    cost_matrix.insert((from, to), weight);
                }
                if left_set.contains(&to) && right_set.contains(&from) {
                    cost_matrix.insert((to, from), weight);
                }
            }
        }

        Ok(cost_matrix)
    }
}

/// 편의 함수: Hungarian 최소 비용 매칭
pub async fn hungarian(engine: &GraphEngine) -> Result<HungarianResult> {
    Hungarian::new(engine).find_matching().await
}

/// 편의 함수: 최소 비용 매칭
pub async fn min_cost_matching(
    engine: &GraphEngine,
    weight_property: &str,
) -> Result<HungarianResult> {
    let config = HungarianConfig::default()
        .with_weight_property(weight_property)
        .with_maximize(false);
    Hungarian::with_config(engine, config).find_matching().await
}

/// 편의 함수: 최대 가중치 매칭
pub async fn max_weight_matching(
    engine: &GraphEngine,
    weight_property: &str,
) -> Result<HungarianResult> {
    let config = HungarianConfig::default()
        .with_weight_property(weight_property)
        .with_maximize(true);
    Hungarian::with_config(engine, config).find_matching().await
}
