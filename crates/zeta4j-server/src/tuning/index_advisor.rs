// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 인덱스 추천 모듈

#![allow(dead_code)]

use std::collections::HashMap;

/// 쿼리 패턴
#[derive(Debug, Clone)]
pub struct QueryPattern {
    /// 대상 라벨
    pub label: Option<String>,
    /// 조회된 속성들
    pub properties: Vec<String>,
    /// 조회 빈도
    pub frequency: usize,
    /// 평균 스캔 행 수
    pub avg_scan_rows: f64,
}

impl QueryPattern {
    /// 새 QueryPattern 생성
    pub fn new(label: Option<String>, properties: Vec<String>) -> Self {
        Self {
            label,
            properties,
            frequency: 1,
            avg_scan_rows: 0.0,
        }
    }

    /// 패턴 키 생성 (중복 감지용)
    pub fn key(&self) -> String {
        format!(
            "{}:{}",
            self.label.as_deref().unwrap_or("*"),
            self.properties.join(",")
        )
    }
}

/// 추천 인덱스 타입
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecommendedIndexType {
    /// B-Tree 인덱스 (일반 속성)
    BTree,
    /// 전문 검색 인덱스
    Fulltext,
    /// 공간 인덱스
    Spatial,
    /// 복합 인덱스
    Composite,
}

impl RecommendedIndexType {
    /// 인덱스 타입 이름 반환
    pub fn name(&self) -> &str {
        match self {
            RecommendedIndexType::BTree => "btree",
            RecommendedIndexType::Fulltext => "fulltext",
            RecommendedIndexType::Spatial => "spatial",
            RecommendedIndexType::Composite => "composite",
        }
    }
}

/// 인덱스 추천
#[derive(Debug, Clone)]
pub struct IndexRecommendation {
    /// 대상 라벨
    pub label: String,
    /// 대상 속성들
    pub properties: Vec<String>,
    /// 추천 인덱스 타입
    pub index_type: RecommendedIndexType,
    /// 예상 성능 향상 배수
    pub estimated_improvement: f64,
    /// 추천 이유
    pub reason: String,
    /// 우선순위 점수 (높을수록 중요)
    pub priority_score: f64,
}

impl IndexRecommendation {
    /// Cypher 인덱스 생성 문 반환
    pub fn to_cypher(&self) -> String {
        match self.index_type {
            RecommendedIndexType::BTree | RecommendedIndexType::Composite => {
                format!(
                    "CREATE INDEX ON :{}({})",
                    self.label,
                    self.properties.join(", ")
                )
            }
            RecommendedIndexType::Fulltext => {
                format!(
                    "CREATE FULLTEXT INDEX ON :{}({})",
                    self.label,
                    self.properties.join(", ")
                )
            }
            RecommendedIndexType::Spatial => {
                format!(
                    "CREATE SPATIAL INDEX ON :{}({})",
                    self.label,
                    self.properties.first().unwrap_or(&"location".to_string())
                )
            }
        }
    }
}

/// 인덱스 사용 통계
#[derive(Debug, Clone, Default)]
struct IndexUsage {
    /// 인덱스 이름
    name: String,
    /// 사용 횟수
    usage_count: usize,
    /// 마지막 사용 시간 (타임스탬프)
    last_used: u64,
}

/// 인덱스 추천기
pub struct IndexAdvisor {
    /// 쿼리 패턴 로그
    patterns: HashMap<String, QueryPattern>,
    /// 기존 인덱스 목록
    existing_indexes: Vec<String>,
    /// 인덱스 사용 통계
    index_usage: HashMap<String, IndexUsage>,
    /// 추천 임계값 설정
    config: IndexAdvisorConfig,
}

/// 인덱스 추천기 설정
#[derive(Debug, Clone)]
pub struct IndexAdvisorConfig {
    /// 추천을 위한 최소 쿼리 빈도
    pub min_frequency: usize,
    /// 추천을 위한 최소 스캔 행 수
    pub min_scan_rows: f64,
    /// 미사용 인덱스 탐지 기간 (초)
    pub unused_threshold_secs: u64,
    /// 최대 추천 개수
    pub max_recommendations: usize,
}

impl Default for IndexAdvisorConfig {
    fn default() -> Self {
        Self {
            min_frequency: 10,
            min_scan_rows: 1000.0,
            unused_threshold_secs: 86400 * 7, // 7일
            max_recommendations: 10,
        }
    }
}

impl IndexAdvisor {
    /// 새 IndexAdvisor 생성
    pub fn new(config: IndexAdvisorConfig) -> Self {
        Self {
            patterns: HashMap::new(),
            existing_indexes: Vec::new(),
            index_usage: HashMap::new(),
            config,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(IndexAdvisorConfig::default())
    }

    /// 쿼리 패턴 기록
    pub fn record_query(&mut self, pattern: QueryPattern) {
        let key = pattern.key();

        if let Some(existing) = self.patterns.get_mut(&key) {
            existing.frequency += pattern.frequency;
            // 이동 평균으로 avg_scan_rows 갱신
            existing.avg_scan_rows = (existing.avg_scan_rows * 0.9) + (pattern.avg_scan_rows * 0.1);
        } else {
            self.patterns.insert(key, pattern);
        }
    }

    /// 기존 인덱스 등록
    pub fn register_existing_index(&mut self, index_name: String) {
        if !self.existing_indexes.contains(&index_name) {
            self.existing_indexes.push(index_name.clone());
            self.index_usage.insert(index_name, IndexUsage::default());
        }
    }

    /// 인덱스 사용 기록
    pub fn record_index_usage(&mut self, index_name: &str) {
        if let Some(usage) = self.index_usage.get_mut(index_name) {
            usage.usage_count += 1;
            usage.last_used = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }
    }

    /// 인덱스 추천 생성
    pub fn recommend(&self) -> Vec<IndexRecommendation> {
        let mut recommendations = Vec::new();

        for pattern in self.patterns.values() {
            // 빈도 및 스캔 행 수 임계값 확인
            if pattern.frequency < self.config.min_frequency {
                continue;
            }
            if pattern.avg_scan_rows < self.config.min_scan_rows {
                continue;
            }

            // 라벨이 있는 패턴만 처리
            let label = match &pattern.label {
                Some(l) => l.clone(),
                None => continue,
            };

            // 속성이 있는 패턴만 처리
            if pattern.properties.is_empty() {
                continue;
            }

            // 이미 인덱스가 있는지 확인
            let index_key = format!("{}_{}", label, pattern.properties.join("_"));
            if self
                .existing_indexes
                .iter()
                .any(|idx| idx.contains(&index_key))
            {
                continue;
            }

            // 인덱스 타입 결정
            let (index_type, reason) = self.determine_index_type(pattern);

            // 예상 성능 향상 계산
            let estimated_improvement = self.estimate_improvement(pattern);

            // 우선순위 점수 계산
            let priority_score = (pattern.frequency as f64) * estimated_improvement;

            recommendations.push(IndexRecommendation {
                label,
                properties: pattern.properties.clone(),
                index_type,
                estimated_improvement,
                reason,
                priority_score,
            });
        }

        // 우선순위 순으로 정렬
        recommendations.sort_by(|a, b| b.priority_score.total_cmp(&a.priority_score));

        // 최대 개수만큼 반환
        recommendations.truncate(self.config.max_recommendations);
        recommendations
    }

    /// 인덱스 타입 결정
    fn determine_index_type(&self, pattern: &QueryPattern) -> (RecommendedIndexType, String) {
        // 복합 인덱스: 2개 이상의 속성
        if pattern.properties.len() > 1 {
            return (
                RecommendedIndexType::Composite,
                format!(
                    "Query using {} properties together occurred {} times",
                    pattern.properties.len(),
                    pattern.frequency
                ),
            );
        }

        // 단일 속성
        let prop = &pattern.properties[0];

        // 공간 데이터 추정 (속성 이름 기반)
        if prop.contains("location")
            || prop.contains("point")
            || prop.contains("geo")
            || prop.contains("coordinate")
        {
            return (
                RecommendedIndexType::Spatial,
                format!("Index recommended for spatial property '{}'", prop),
            );
        }

        // 텍스트 검색 추정 (평균 스캔 행 수가 많은 경우)
        if pattern.avg_scan_rows > 10000.0 && prop.contains("text")
            || prop.contains("description")
            || prop.contains("content")
        {
            return (
                RecommendedIndexType::Fulltext,
                format!("Fulltext index recommended for text property '{}'", prop),
            );
        }

        // 기본 B-Tree
        (
            RecommendedIndexType::BTree,
            format!(
                "Query on property '{}' occurred {} times, avg {:.0} rows scanned",
                prop, pattern.frequency, pattern.avg_scan_rows
            ),
        )
    }

    /// 예상 성능 향상 계산
    fn estimate_improvement(&self, pattern: &QueryPattern) -> f64 {
        // 간단한 휴리스틱: 스캔 행 수가 많을수록 향상 효과 큼
        // 인덱스 사용 시 O(log n) vs 전체 스캔 O(n)
        let scan_rows = pattern.avg_scan_rows;
        if scan_rows <= 10.0 {
            1.0 // 향상 없음
        } else if scan_rows <= 100.0 {
            2.0
        } else if scan_rows <= 1000.0 {
            5.0
        } else if scan_rows <= 10000.0 {
            10.0
        } else {
            20.0
        }
    }

    /// 미사용 인덱스 탐지
    pub fn find_unused_indexes(&self) -> Vec<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut unused = Vec::new();

        for (name, usage) in &self.index_usage {
            // 한 번도 사용되지 않았거나 오래된 경우
            if usage.usage_count == 0
                || now.saturating_sub(usage.last_used) > self.config.unused_threshold_secs
            {
                unused.push(name.clone());
            }
        }

        unused
    }

    /// 패턴 통계 반환
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// 모든 패턴 반환
    pub fn patterns(&self) -> Vec<&QueryPattern> {
        self.patterns.values().collect()
    }
}
