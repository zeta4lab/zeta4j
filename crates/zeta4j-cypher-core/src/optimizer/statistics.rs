// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 쿼리 최적화를 위한 통계 수집 모듈
//!
//! 그래프 데이터의 통계 정보를 수집하여 카디널리티 추정 및 비용 계산에 활용합니다.

use std::collections::HashMap;
use std::sync::Arc;
use zeta4j_graph::core::{PropertyContainer, Value};
use zeta4j_graph::engine::GraphEngine;

/// 프로퍼티별 통계
#[derive(Debug, Clone, Default)]
pub struct PropertyStatistics {
    /// 프로퍼티 이름
    pub property: String,
    /// 서로 다른 값의 개수
    pub distinct_count: u64,
    /// NULL 값 개수
    pub null_count: u64,
    /// 전체 값 개수
    pub total_count: u64,
    /// 최소값 (숫자인 경우)
    pub min_value: Option<f64>,
    /// 최대값 (숫자인 경우)
    pub max_value: Option<f64>,
    /// 히스토그램 (선택적)
    pub histogram: Option<Histogram>,
}

/// 등폭 히스토그램 (Equi-width Histogram)
#[derive(Debug, Clone)]
pub struct Histogram {
    /// 버킷 수
    pub num_buckets: usize,
    /// 각 버킷의 빈도수
    pub buckets: Vec<u64>,
    /// 히스토그램 최소값
    pub min_value: f64,
    /// 히스토그램 최대값
    pub max_value: f64,
}

impl Histogram {
    /// 새 히스토그램 생성
    pub fn new(values: &[f64], num_buckets: usize) -> Option<Self> {
        if values.is_empty() || num_buckets == 0 {
            return None;
        }

        let min_value = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_value = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        // 모든 값이 동일한 경우
        if (max_value - min_value).abs() < f64::EPSILON {
            return Some(Self {
                num_buckets: 1,
                buckets: vec![values.len() as u64],
                min_value,
                max_value: max_value + 1.0, // 범위를 만들기 위해
            });
        }

        let bucket_width = (max_value - min_value) / num_buckets as f64;
        let mut buckets = vec![0u64; num_buckets];

        for &value in values {
            let bucket_idx = ((value - min_value) / bucket_width).floor() as usize;
            let bucket_idx = bucket_idx.min(num_buckets - 1); // 최대값은 마지막 버킷에
            buckets[bucket_idx] += 1;
        }

        Some(Self {
            num_buckets,
            buckets,
            min_value,
            max_value,
        })
    }

    /// 범위 내 비율 추정 (lower_bound, upper_bound)
    pub fn estimate_range_fraction(&self, lower: Option<f64>, upper: Option<f64>) -> f64 {
        let effective_lower = lower.unwrap_or(self.min_value);
        let effective_upper = upper.unwrap_or(self.max_value);

        // 범위가 히스토그램 범위를 벗어나는 경우
        if effective_upper < self.min_value || effective_lower > self.max_value {
            return 0.0;
        }

        let clamped_lower = effective_lower.max(self.min_value);
        let clamped_upper = effective_upper.min(self.max_value);

        if clamped_lower >= clamped_upper {
            return 0.0;
        }

        let bucket_width = (self.max_value - self.min_value) / self.num_buckets as f64;
        let total_count: u64 = self.buckets.iter().sum();

        if total_count == 0 {
            return 0.0;
        }

        let mut count = 0.0;

        for (i, &bucket_count) in self.buckets.iter().enumerate() {
            let bucket_start = self.min_value + i as f64 * bucket_width;
            let bucket_end = bucket_start + bucket_width;

            // 버킷과 범위의 교집합 계산
            let overlap_start = clamped_lower.max(bucket_start);
            let overlap_end = clamped_upper.min(bucket_end);

            if overlap_start < overlap_end {
                // 버킷 내 비율 계산 (균등 분포 가정)
                let overlap_fraction = (overlap_end - overlap_start) / bucket_width;
                count += bucket_count as f64 * overlap_fraction;
            }
        }

        count / total_count as f64
    }
}

impl PropertyStatistics {
    /// 선택도 계산 (특정 값이 선택될 확률)
    /// distinct_count가 0이면 1.0 반환 (모두 선택)
    pub fn selectivity(&self) -> f64 {
        if self.distinct_count == 0 {
            1.0
        } else {
            1.0 / self.distinct_count as f64
        }
    }

    /// NULL 비율
    pub fn null_ratio(&self) -> f64 {
        if self.total_count == 0 {
            0.0
        } else {
            self.null_count as f64 / self.total_count as f64
        }
    }

    /// 범위 조건 선택도 추정
    /// lower: 하한값 (None이면 무한대 하한)
    /// upper: 상한값 (None이면 무한대 상한)
    /// lower_inclusive, upper_inclusive: 경계 포함 여부
    pub fn estimate_range_selectivity(
        &self,
        lower: Option<f64>,
        upper: Option<f64>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    ) -> f64 {
        // 1. 히스토그램이 있으면 사용
        if let Some(ref histogram) = self.histogram {
            let fraction = histogram.estimate_range_fraction(lower, upper);
            // inclusive 조정 (미미한 차이)
            let adjustment = if !lower_inclusive || !upper_inclusive {
                0.99
            } else {
                1.0
            };
            return (fraction * adjustment).clamp(0.001, 1.0);
        }

        // 2. min/max 값이 있으면 선형 보간 사용
        if let (Some(min), Some(max)) = (self.min_value, self.max_value) {
            if (max - min).abs() < f64::EPSILON {
                // 모든 값이 동일
                return match (lower, upper) {
                    (Some(l), Some(u)) if l <= min && min <= u => 1.0,
                    (Some(l), None) if l <= min => 1.0,
                    (None, Some(u)) if min <= u => 1.0,
                    (None, None) => 1.0,
                    _ => 0.0,
                };
            }

            let effective_lower = lower.unwrap_or(min).max(min);
            let effective_upper = upper.unwrap_or(max).min(max);

            if effective_lower > effective_upper {
                return 0.0;
            }

            let range_fraction = (effective_upper - effective_lower) / (max - min);
            return range_fraction.clamp(0.001, 1.0);
        }

        // 3. 기본값: PostgreSQL 스타일 추정
        match (lower, upper) {
            (Some(_), Some(_)) => 0.25,                // BETWEEN 스타일: 25%
            (Some(_), None) | (None, Some(_)) => 0.33, // 단일 비교: 33%
            (None, None) => 1.0,                       // 범위 없음: 전체
        }
    }
}

/// 레이블별 통계
#[derive(Debug, Clone, Default)]
pub struct LabelStatistics {
    /// 레이블 이름
    pub label: String,
    /// 해당 레이블을 가진 노드 수
    pub node_count: u64,
    /// 프로퍼티별 통계
    pub property_stats: HashMap<String, PropertyStatistics>,
}

impl LabelStatistics {
    /// 새 레이블 통계 생성
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            node_count: 0,
            property_stats: HashMap::new(),
        }
    }
}

/// 관계 타입별 통계
#[derive(Debug, Clone, Default)]
pub struct RelationshipStatistics {
    /// 관계 타입
    pub rel_type: String,
    /// 전체 관계 수
    pub count: u64,
    /// 평균 out-degree (노드당 평균 나가는 관계 수)
    pub avg_out_degree: f64,
    /// 평균 in-degree (노드당 평균 들어오는 관계 수)
    pub avg_in_degree: f64,
}

impl RelationshipStatistics {
    /// 새 관계 통계 생성
    pub fn new(rel_type: impl Into<String>) -> Self {
        Self {
            rel_type: rel_type.into(),
            count: 0,
            avg_out_degree: 0.0,
            avg_in_degree: 0.0,
        }
    }
}

/// 인덱스 통계
#[derive(Debug, Clone)]
pub struct IndexStatistics {
    /// 인덱스 이름
    pub name: String,
    /// 대상 레이블
    pub label: String,
    /// 대상 프로퍼티
    pub property: String,
    /// 인덱스된 항목 수 (카디널리티)
    pub cardinality: u64,
}

/// 전체 통계 관리자
#[derive(Debug, Default)]
pub struct StatisticsManager {
    /// 레이블별 통계
    label_stats: HashMap<String, LabelStatistics>,
    /// 관계 타입별 통계
    rel_stats: HashMap<String, RelationshipStatistics>,
    /// 인덱스 통계
    index_stats: HashMap<String, IndexStatistics>,
    /// 전체 노드 수
    pub total_nodes: u64,
    /// 전체 엣지 수
    pub total_edges: u64,
    /// 마지막 갱신 시각 (밀리초)
    pub last_refresh: u64,
}

impl StatisticsManager {
    /// 새 통계 관리자 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 엔진에서 통계 수집
    pub async fn refresh(&mut self, engine: &Arc<GraphEngine>) {
        self.total_nodes = engine.node_count().await.unwrap_or(0) as u64;
        self.total_edges = engine.edge_count().await as u64;

        // 레이블별 통계 수집
        self.collect_label_stats(engine).await;

        // 관계 타입별 통계 수집
        self.collect_relationship_stats(engine).await;

        // 인덱스 통계 수집
        self.collect_index_stats(engine);

        // 갱신 시각 기록
        self.last_refresh = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }

    /// 레이블별 통계 수집
    async fn collect_label_stats(&mut self, engine: &Arc<GraphEngine>) {
        self.label_stats.clear();

        // 모든 노드를 순회하며 레이블별 통계 수집
        let node_ids = engine.all_node_ids().await;
        let mut label_nodes: HashMap<String, Vec<u64>> = HashMap::new();

        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                for label in node.labels() {
                    label_nodes
                        .entry(label.to_string())
                        .or_default()
                        .push(*node_id);
                }
            }
        }

        // 레이블별 노드 수 저장
        for (label, nodes) in label_nodes {
            let mut stats = LabelStatistics::new(&label);
            stats.node_count = nodes.len() as u64;

            // 프로퍼티 통계 수집 (샘플링)
            self.collect_property_stats_for_label(engine, &label, &nodes, &mut stats)
                .await;

            self.label_stats.insert(label, stats);
        }
    }

    /// 특정 레이블의 프로퍼티 통계 수집
    async fn collect_property_stats_for_label(
        &self,
        engine: &Arc<GraphEngine>,
        _label: &str,
        node_ids: &[u64],
        stats: &mut LabelStatistics,
    ) {
        // 프로퍼티별 값 수집
        let mut property_values: HashMap<String, Vec<Option<Value>>> = HashMap::new();

        for node_id in node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                // 노드의 모든 프로퍼티 수집
                for (key, value) in node.properties().iter() {
                    property_values
                        .entry(key.clone())
                        .or_default()
                        .push(Some(value.clone()));
                }
            }
        }

        // 프로퍼티별 통계 계산
        for (property, values) in property_values {
            let total_count = values.len() as u64;
            let null_count = values
                .iter()
                .filter(|v| v.is_none() || matches!(v, Some(Value::Null)))
                .count() as u64;

            // distinct 값 계산 (해시셋 사용)
            let distinct_values: std::collections::HashSet<String> = values
                .iter()
                .filter_map(|v| v.as_ref())
                .filter(|v| !matches!(v, Value::Null))
                .map(|v| format!("{:?}", v))
                .collect();

            // 숫자 값 추출 (min/max 및 히스토그램용)
            let numeric_values: Vec<f64> = values
                .iter()
                .filter_map(|v| v.as_ref())
                .filter_map(Self::value_to_f64)
                .collect();

            let (min_value, max_value, histogram) = if !numeric_values.is_empty() {
                let min = numeric_values.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = numeric_values
                    .iter()
                    .cloned()
                    .fold(f64::NEG_INFINITY, f64::max);

                // 충분한 데이터가 있으면 히스토그램 생성 (10개 버킷)
                let hist = if numeric_values.len() >= 10 {
                    Histogram::new(&numeric_values, 10)
                } else {
                    None
                };

                (Some(min), Some(max), hist)
            } else {
                (None, None, None)
            };

            let prop_stats = PropertyStatistics {
                property: property.clone(),
                distinct_count: distinct_values.len() as u64,
                null_count,
                total_count,
                min_value,
                max_value,
                histogram,
            };

            stats.property_stats.insert(property, prop_stats);
        }
    }

    /// Value를 f64로 변환 (숫자 타입만)
    fn value_to_f64(value: &Value) -> Option<f64> {
        match value {
            Value::Integer(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// 관계 타입별 통계 수집
    async fn collect_relationship_stats(&mut self, engine: &Arc<GraphEngine>) {
        self.rel_stats.clear();

        let edge_ids = engine.all_edge_ids().await;
        let mut rel_counts: HashMap<String, u64> = HashMap::new();
        let mut out_degree: HashMap<u64, HashMap<String, u64>> = HashMap::new();
        let mut in_degree: HashMap<u64, HashMap<String, u64>> = HashMap::new();

        for edge_id in &edge_ids {
            if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
                let rel_type = edge.relation_type().to_string();
                *rel_counts.entry(rel_type.clone()).or_default() += 1;

                // out-degree 계산
                out_degree
                    .entry(edge.from_node())
                    .or_default()
                    .entry(rel_type.clone())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);

                // in-degree 계산
                in_degree
                    .entry(edge.to_node())
                    .or_default()
                    .entry(rel_type)
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
            }
        }

        // 평균 degree 계산
        let total_nodes = self.total_nodes.max(1) as f64;

        for (rel_type, count) in rel_counts {
            let avg_out = out_degree
                .values()
                .filter_map(|m| m.get(&rel_type))
                .map(|&c| c as f64)
                .sum::<f64>()
                / total_nodes;

            let avg_in = in_degree
                .values()
                .filter_map(|m| m.get(&rel_type))
                .map(|&c| c as f64)
                .sum::<f64>()
                / total_nodes;

            let stats = RelationshipStatistics {
                rel_type: rel_type.clone(),
                count,
                avg_out_degree: avg_out,
                avg_in_degree: avg_in,
            };

            self.rel_stats.insert(rel_type, stats);
        }
    }

    /// 인덱스 통계 수집
    fn collect_index_stats(&mut self, engine: &Arc<GraphEngine>) {
        self.index_stats.clear();

        // SecondaryIndexManager에서 인덱스 정보 가져오기
        let indexes = engine.list_property_indexes();

        for (name, label, properties) in indexes {
            let property = properties.first().cloned().unwrap_or_default();

            // 인덱스 카디널리티 추정 (레이블 노드 수 사용)
            let cardinality = self.label_count(&label);

            let stats = IndexStatistics {
                name: name.clone(),
                label,
                property,
                cardinality,
            };

            self.index_stats.insert(name, stats);
        }
    }

    // ========== 조회 메서드 ==========

    /// 전체 노드 수
    pub fn total_node_count(&self) -> u64 {
        self.total_nodes
    }

    /// 전체 엣지 수
    pub fn total_edge_count(&self) -> u64 {
        self.total_edges
    }

    /// 레이블별 노드 수
    pub fn label_count(&self, label: &str) -> u64 {
        self.label_stats
            .get(label)
            .map(|s| s.node_count)
            .unwrap_or(0)
    }

    /// 레이블 통계 조회
    pub fn get_label_stats(&self, label: &str) -> Option<&LabelStatistics> {
        self.label_stats.get(label)
    }

    /// 관계 타입 통계 조회
    pub fn get_relationship_stats(&self, rel_type: &str) -> Option<&RelationshipStatistics> {
        self.rel_stats.get(rel_type)
    }

    /// 인덱스 통계 조회
    pub fn get_index_stats(&self, name: &str) -> Option<&IndexStatistics> {
        self.index_stats.get(name)
    }

    /// 인덱스 카디널리티 조회 (레이블+프로퍼티 기반)
    pub fn index_cardinality(&self, label: &str, property: &str) -> Option<u64> {
        self.index_stats
            .values()
            .find(|s| s.label == label && s.property == property)
            .map(|s| s.cardinality)
    }

    /// 프로퍼티 선택도 조회 (0.0 ~ 1.0)
    /// 특정 값이 선택될 확률을 반환
    pub fn selectivity(&self, label: &str, property: &str) -> f64 {
        self.label_stats
            .get(label)
            .and_then(|ls| ls.property_stats.get(property))
            .map(|ps| ps.selectivity())
            .unwrap_or(0.1) // 기본 선택도 10%
    }

    /// 등호 조건 선택도 (n.prop = value)
    pub fn equality_selectivity(&self, label: &str, property: &str) -> f64 {
        self.selectivity(label, property)
    }

    /// 범위 조건 선택도 (n.prop > value, n.prop < value 등)
    /// 기본적으로 33% 선택도 사용 (PostgreSQL 기본값)
    pub fn range_selectivity(&self, label: &str, property: &str) -> f64 {
        // 프로퍼티 통계가 있으면 기본 추정값 사용
        self.label_stats
            .get(label)
            .and_then(|ls| ls.property_stats.get(property))
            .map(|ps| ps.estimate_range_selectivity(None, None, true, true))
            .unwrap_or(0.33)
    }

    /// 범위 조건 선택도 추정 (상세 버전)
    /// lower: 하한값 (None이면 무한대 하한)
    /// upper: 상한값 (None이면 무한대 상한)
    pub fn estimate_range_selectivity(
        &self,
        label: &str,
        property: &str,
        lower: Option<f64>,
        upper: Option<f64>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    ) -> f64 {
        self.label_stats
            .get(label)
            .and_then(|ls| ls.property_stats.get(property))
            .map(|ps| ps.estimate_range_selectivity(lower, upper, lower_inclusive, upper_inclusive))
            .unwrap_or_else(|| {
                // 통계 없음: 기본 추정
                match (lower, upper) {
                    (Some(_), Some(_)) => 0.25,
                    (Some(_), None) | (None, Some(_)) => 0.33,
                    (None, None) => 1.0,
                }
            })
    }

    /// 관계 확장 시 예상 행 수
    pub fn expand_estimate(&self, rel_type: Option<&str>, direction_out: bool) -> f64 {
        match rel_type {
            Some(rt) => {
                self.rel_stats
                    .get(rt)
                    .map(|s| {
                        if direction_out {
                            s.avg_out_degree
                        } else {
                            s.avg_in_degree
                        }
                    })
                    .unwrap_or(2.0) // 기본 degree 2
            }
            None => {
                // 모든 관계 타입의 평균
                if self.rel_stats.is_empty() {
                    2.0
                } else {
                    let total: f64 = self
                        .rel_stats
                        .values()
                        .map(|s| {
                            if direction_out {
                                s.avg_out_degree
                            } else {
                                s.avg_in_degree
                            }
                        })
                        .sum();
                    total / self.rel_stats.len() as f64
                }
            }
        }
    }

    /// 모든 레이블 목록
    pub fn get_all_labels(&self) -> Vec<&str> {
        self.label_stats.keys().map(|s| s.as_str()).collect()
    }

    /// 모든 관계 타입 목록
    pub fn all_relationship_types(&self) -> Vec<&str> {
        self.rel_stats.keys().map(|s| s.as_str()).collect()
    }

    // ========== 통계 업데이트 (테스트/수동 설정용) ==========

    /// 레이블 노드 수 설정
    pub fn update_label_count(&mut self, label: &str, count: u64) {
        let stats = self
            .label_stats
            .entry(label.to_string())
            .or_insert_with(|| LabelStatistics::new(label));
        stats.node_count = count;
    }

    /// 관계 타입 통계 설정
    pub fn update_relationship_stats(
        &mut self,
        rel_type: &str,
        count: u64,
        avg_out: f64,
        avg_in: f64,
    ) {
        let stats = self
            .rel_stats
            .entry(rel_type.to_string())
            .or_insert_with(|| RelationshipStatistics::new(rel_type));
        stats.count = count;
        stats.avg_out_degree = avg_out;
        stats.avg_in_degree = avg_in;
    }

    /// 전체 노드 수 설정
    pub fn set_total_nodes(&mut self, count: u64) {
        self.total_nodes = count;
    }

    /// 전체 엣지 수 설정
    pub fn set_total_edges(&mut self, count: u64) {
        self.total_edges = count;
    }
}
