//! 패턴 매칭 최적화
//!
//! 그래프 DB의 핵심 연산인 MATCH 패턴의 실행 순서를 최적화합니다.
//!
//! # 최적화 전략
//!
//! 1. **시작점 선택**: 가장 선택적인 노드부터 시작
//! 2. **Expand 순서**: 비용이 낮은 관계 탐색 우선
//! 3. **방향 최적화**: 실제 탐색 방향 결정
//! 4. **필터 푸시다운**: WHERE 조건을 일찍 적용

use indexmap::IndexMap;

use super::cost_model::CostModel;
use super::statistics::StatisticsManager;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use zeta4j_graph::core::Value;

/// 관계 방향
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    /// 나가는 방향: (a)-[r]->(b)
    Outgoing,
    /// 들어오는 방향: (a)<-[r]-(b)
    Incoming,
    /// 양방향: (a)-[r]-(b)
    Both,
}

impl Direction {
    /// 방향 반전
    pub fn reverse(&self) -> Self {
        match self {
            Direction::Outgoing => Direction::Incoming,
            Direction::Incoming => Direction::Outgoing,
            Direction::Both => Direction::Both,
        }
    }

    /// 방향 이름
    pub fn name(&self) -> &'static str {
        match self {
            Direction::Outgoing => "OUTGOING",
            Direction::Incoming => "INCOMING",
            Direction::Both => "BOTH",
        }
    }
}

/// 노드 패턴
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePattern {
    /// 변수명 (없을 수 있음)
    pub variable: Option<String>,
    /// 레이블 목록 (복수 가능)
    pub labels: Vec<String>,
    /// 속성 조건 (인라인)
    pub properties: IndexMap<String, Value>,
}

impl NodePattern {
    /// 새 노드 패턴 생성
    pub fn new() -> Self {
        Self {
            variable: None,
            labels: Vec::new(),
            properties: IndexMap::new(),
        }
    }

    /// 변수명 설정
    pub fn with_variable(mut self, var: &str) -> Self {
        self.variable = Some(var.to_string());
        self
    }

    /// 레이블 추가
    pub fn with_label(mut self, label: &str) -> Self {
        self.labels.push(label.to_string());
        self
    }

    /// 속성 조건 추가
    pub fn with_property(mut self, key: &str, value: Value) -> Self {
        self.properties.insert(key.to_string(), value);
        self
    }

    /// 레이블이 있는지 확인
    pub fn has_labels(&self) -> bool {
        !self.labels.is_empty()
    }

    /// 속성 조건이 있는지 확인
    pub fn has_properties(&self) -> bool {
        !self.properties.is_empty()
    }
}

impl Default for NodePattern {
    fn default() -> Self {
        Self::new()
    }
}

/// 관계 패턴
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipPattern {
    /// 변수명 (없을 수 있음)
    pub variable: Option<String>,
    /// 관계 타입 목록
    pub rel_types: Vec<String>,
    /// 방향
    pub direction: Direction,
    /// 가변 길이 (min, max) - None이면 정확히 1홉
    pub var_length: Option<(u32, u32)>,
    /// 속성 조건
    pub properties: IndexMap<String, Value>,
}

impl RelationshipPattern {
    /// 새 관계 패턴 생성
    pub fn new(direction: Direction) -> Self {
        Self {
            variable: None,
            rel_types: Vec::new(),
            direction,
            var_length: None,
            properties: IndexMap::new(),
        }
    }

    /// 변수명 설정
    pub fn with_variable(mut self, var: &str) -> Self {
        self.variable = Some(var.to_string());
        self
    }

    /// 관계 타입 추가
    pub fn with_type(mut self, rel_type: &str) -> Self {
        self.rel_types.push(rel_type.to_string());
        self
    }

    /// 가변 길이 설정
    pub fn with_var_length(mut self, min: u32, max: u32) -> Self {
        self.var_length = Some((min, max));
        self
    }

    /// 가변 길이인지 확인
    pub fn is_var_length(&self) -> bool {
        self.var_length.is_some()
    }

    /// 관계 타입이 지정되었는지 확인
    pub fn has_types(&self) -> bool {
        !self.rel_types.is_empty()
    }
}

/// 경로 세그먼트 (관계 + 대상 노드)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathSegment {
    /// 관계 패턴
    pub relationship: RelationshipPattern,
    /// 대상 노드 패턴
    pub node: NodePattern,
}

/// 경로 패턴 (MATCH 패턴 전체)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathPattern {
    /// 시작 노드
    pub start: NodePattern,
    /// 세그먼트 목록 [(관계, 노드), ...]
    pub segments: Vec<PathSegment>,
}

impl PathPattern {
    /// 새 경로 패턴 생성
    pub fn new(start: NodePattern) -> Self {
        Self {
            start,
            segments: Vec::new(),
        }
    }

    /// 세그먼트 추가
    pub fn add_segment(&mut self, relationship: RelationshipPattern, node: NodePattern) {
        self.segments.push(PathSegment { relationship, node });
    }

    /// 빌더 스타일 세그먼트 추가
    pub fn with_segment(mut self, relationship: RelationshipPattern, node: NodePattern) -> Self {
        self.add_segment(relationship, node);
        self
    }

    /// 노드 수 (시작 노드 + 세그먼트 노드들)
    pub fn node_count(&self) -> usize {
        1 + self.segments.len()
    }

    /// 인덱스로 노드 가져오기
    pub fn get_node(&self, index: usize) -> Option<&NodePattern> {
        if index == 0 {
            Some(&self.start)
        } else {
            self.segments.get(index - 1).map(|s| &s.node)
        }
    }

    /// 모든 노드 순회
    pub fn nodes(&self) -> impl Iterator<Item = &NodePattern> {
        std::iter::once(&self.start).chain(self.segments.iter().map(|s| &s.node))
    }

    /// 모든 변수 수집
    pub fn collect_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();

        if let Some(v) = &self.start.variable {
            vars.push(v.clone());
        }

        for seg in &self.segments {
            if let Some(v) = &seg.relationship.variable {
                vars.push(v.clone());
            }
            if let Some(v) = &seg.node.variable {
                vars.push(v.clone());
            }
        }

        vars
    }
}

/// Expand 단계
#[derive(Debug, Clone)]
pub struct ExpandStep {
    /// 원본 노드 인덱스
    pub from_index: usize,
    /// 대상 노드 인덱스
    pub to_index: usize,
    /// 관계 타입들
    pub rel_types: Vec<String>,
    /// 원래 방향
    pub original_direction: Direction,
    /// 실제 탐색 방향 (최적화 후)
    pub actual_direction: Direction,
    /// 가변 길이
    pub var_length: Option<(u32, u32)>,
    /// 예상 결과 행 수
    pub estimated_rows: u64,
    /// 예상 비용
    pub cost: f64,
}

/// Expand 계획 (최적화된 탐색 순서)
#[derive(Debug, Clone)]
pub struct ExpandPlan {
    /// 시작 노드 인덱스
    pub start_index: usize,
    /// 시작 노드 스캔 비용
    pub start_scan_cost: f64,
    /// 시작 노드 예상 행 수
    pub start_rows: u64,
    /// Expand 단계 목록 (순서대로 실행)
    pub steps: Vec<ExpandStep>,
    /// 총 예상 비용
    pub total_cost: f64,
}

impl ExpandPlan {
    /// 새 계획 생성
    pub fn new(start_index: usize, start_scan_cost: f64, start_rows: u64) -> Self {
        Self {
            start_index,
            start_scan_cost,
            start_rows,
            steps: Vec::new(),
            total_cost: start_scan_cost,
        }
    }

    /// 단계 추가
    pub fn add_step(&mut self, step: ExpandStep) {
        self.total_cost += step.cost;
        self.steps.push(step);
    }

    /// 최종 예상 행 수
    pub fn final_rows(&self) -> u64 {
        if let Some(last) = self.steps.last() {
            last.estimated_rows
        } else {
            self.start_rows
        }
    }
}

/// 패턴 최적화기
pub struct PatternOptimizer {
    /// 비용 모델
    cost_model: CostModel,
    /// 기본 레이블 카운트 (통계 없을 때)
    default_label_count: u64,
    /// 기본 관계 차수
    default_degree: f64,
}

impl Default for PatternOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternOptimizer {
    /// 새 최적화기 생성
    pub fn new() -> Self {
        Self {
            cost_model: CostModel::new(),
            default_label_count: 10000,
            default_degree: 10.0,
        }
    }

    /// 비용 모델 설정
    pub fn with_cost_model(mut self, cost_model: CostModel) -> Self {
        self.cost_model = cost_model;
        self
    }

    /// 통계 기반 최적화
    pub fn optimize(&self, pattern: &PathPattern, stats: &StatisticsManager) -> ExpandPlan {
        // 1. 최적 시작점 선택
        let start_index = self.select_start_node(pattern, stats);

        // 2. 시작 노드 비용 계산
        let Some(start_node) = pattern.get_node(start_index) else {
            return ExpandPlan::new(0, 0.0, 0);
        };
        let start_rows = self.estimate_node_rows(start_node, stats);
        let start_cost = self.node_scan_cost(start_node, stats);

        // 3. Expand 순서 결정
        let mut plan = ExpandPlan::new(start_index, start_cost, start_rows);
        self.build_expand_order(pattern, stats, &mut plan);

        plan
    }

    /// 최적 시작점 선택 (가장 선택적인 노드)
    pub fn select_start_node(&self, pattern: &PathPattern, stats: &StatisticsManager) -> usize {
        let mut best_index = 0;
        let mut min_rows = u64::MAX;

        for (i, node) in pattern.nodes().enumerate() {
            let rows = self.estimate_node_rows(node, stats);
            if rows < min_rows {
                min_rows = rows;
                best_index = i;
            }
        }

        best_index
    }

    /// 노드 예상 행 수 추정
    pub fn estimate_node_rows(&self, node: &NodePattern, stats: &StatisticsManager) -> u64 {
        let mut rows = if node.labels.is_empty() {
            stats.total_node_count().max(self.default_label_count)
        } else {
            // 가장 선택적인 레이블 사용
            node.labels
                .iter()
                .map(|l| {
                    let count = stats.label_count(l);
                    if count == 0 {
                        self.default_label_count
                    } else {
                        count
                    }
                })
                .min()
                .unwrap_or(self.default_label_count)
        };

        // 속성 조건이 있으면 선택도 적용
        if node.has_properties() {
            let selectivity = 0.1_f64.powi(node.properties.len() as i32);
            rows = (rows as f64 * selectivity).max(1.0) as u64;
        }

        rows.max(1)
    }

    /// 노드 스캔 비용
    fn node_scan_cost(&self, node: &NodePattern, stats: &StatisticsManager) -> f64 {
        let rows = self.estimate_node_rows(node, stats);

        if node.has_properties() {
            // 인덱스 스캔 가능성
            self.cost_model.index_scan_cost(rows, 0.1)
        } else if node.has_labels() {
            self.cost_model
                .label_scan_cost(rows, stats.total_node_count().max(rows * 10))
        } else {
            self.cost_model.all_nodes_scan_cost(rows)
        }
    }

    /// Expand 순서 결정 (그리디)
    fn build_expand_order(
        &self,
        pattern: &PathPattern,
        stats: &StatisticsManager,
        plan: &mut ExpandPlan,
    ) {
        let n = pattern.node_count();
        if n <= 1 {
            return;
        }

        let mut visited = HashSet::new();
        visited.insert(plan.start_index);

        let mut current_rows = plan.start_rows;

        // 인접 리스트 구성
        let adjacency = self.build_adjacency(pattern);

        // 그리디하게 가장 저렴한 Expand 선택
        while visited.len() < n {
            let mut best_step: Option<ExpandStep> = None;
            let mut min_cost = f64::MAX;

            // 방문한 노드에서 미방문 노드로의 Expand 평가
            for &from in &visited {
                if let Some(neighbors) = adjacency.get(&from) {
                    for &(to, seg_idx) in neighbors {
                        if visited.contains(&to) {
                            continue;
                        }

                        let seg = &pattern.segments[seg_idx];
                        let step = self.create_expand_step(
                            pattern,
                            from,
                            to,
                            seg_idx,
                            seg,
                            current_rows,
                            stats,
                        );

                        if step.cost < min_cost {
                            min_cost = step.cost;
                            best_step = Some(step);
                        }
                    }
                }
            }

            if let Some(step) = best_step {
                visited.insert(step.to_index);
                current_rows = step.estimated_rows;
                plan.add_step(step);
            } else {
                break;
            }
        }
    }

    /// 인접 리스트 구성
    fn build_adjacency(&self, pattern: &PathPattern) -> HashMap<usize, Vec<(usize, usize)>> {
        let mut adj: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();

        for (i, _seg) in pattern.segments.iter().enumerate() {
            let from = i;
            let to = i + 1;

            // 양방향으로 추가 (방향 최적화 위해)
            adj.entry(from).or_default().push((to, i));
            adj.entry(to).or_default().push((from, i));
        }

        adj
    }

    /// Expand 단계 생성
    #[expect(clippy::too_many_arguments)]
    fn create_expand_step(
        &self,
        pattern: &PathPattern,
        from: usize,
        to: usize,
        seg_idx: usize,
        seg: &PathSegment,
        input_rows: u64,
        stats: &StatisticsManager,
    ) -> ExpandStep {
        let rel = &seg.relationship;

        // 방향 결정: from이 seg_idx면 정방향, 아니면 역방향
        let (original_dir, actual_dir) = if from == seg_idx {
            (rel.direction, rel.direction)
        } else {
            (rel.direction, rel.direction.reverse())
        };

        // 관계 차수 추정
        let avg_degree = if rel.has_types() {
            rel.rel_types
                .iter()
                .map(|t| stats.expand_estimate(Some(t), actual_dir == Direction::Outgoing))
                .sum::<f64>()
                / rel.rel_types.len() as f64
        } else {
            stats.expand_estimate(None, actual_dir == Direction::Outgoing)
        };

        let avg_degree = if avg_degree == 0.0 {
            self.default_degree
        } else {
            avg_degree
        };

        // 결과 행 수 추정
        let estimated_rows = if let Some((min, max)) = rel.var_length {
            self.estimate_var_length_rows(input_rows, avg_degree, min, max)
        } else {
            (input_rows as f64 * avg_degree) as u64
        };

        // 대상 노드 조건으로 필터링
        let target_node = if to == 0 {
            &pattern.start
        } else {
            &pattern.segments[to - 1].node
        };

        let filtered_rows = if target_node.has_labels() || target_node.has_properties() {
            let selectivity = 0.1;
            (estimated_rows as f64 * selectivity) as u64
        } else {
            estimated_rows
        };

        // 비용 계산
        let cost = if let Some((min, max)) = rel.var_length {
            self.cost_model
                .var_length_expand_cost(input_rows, avg_degree, min, max)
        } else {
            self.cost_model.expand_cost(input_rows, avg_degree)
        };

        ExpandStep {
            from_index: from,
            to_index: to,
            rel_types: rel.rel_types.clone(),
            original_direction: original_dir,
            actual_direction: actual_dir,
            var_length: rel.var_length,
            estimated_rows: filtered_rows.max(1),
            cost,
        }
    }

    /// 가변 길이 경로 행 수 추정
    fn estimate_var_length_rows(&self, input: u64, degree: f64, min: u32, max: u32) -> u64 {
        let mut total = 0.0;
        let mut current = input as f64;

        for _ in min..=max {
            current *= degree;
            total += current;
        }

        total.max(1.0) as u64
    }
}

// PathPattern을 저장하지 않고 사용하기 위한 헬퍼
impl PatternOptimizer {
    /// 정적 분석 (통계 없이)
    pub fn analyze_static(&self, pattern: &PathPattern) -> ExpandPlan {
        // 기본 통계 사용
        let stats = StatisticsManager::new();
        self.optimize(pattern, &stats)
    }
}

/// CostModel에 그래프 전용 비용 함수 추가
impl CostModel {
    /// VarLength Expand 비용
    pub fn var_length_expand_cost(
        &self,
        input_rows: u64,
        avg_degree: f64,
        min_hops: u32,
        max_hops: u32,
    ) -> f64 {
        let mut total_cost = 0.0;
        let mut current_rows = input_rows as f64;

        for _ in min_hops..=max_hops {
            current_rows *= avg_degree;
            total_cost += current_rows * self.cpu_tuple_cost + self.random_page_cost;
        }

        total_cost
    }
}
