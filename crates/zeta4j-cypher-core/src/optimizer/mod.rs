// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 쿼리 최적화 모듈
//!
//! 비용 기반 최적화(Cost-Based Optimization)를 위한 구성요소를 제공합니다.
//!
//! # 구성요소
//!
//! - `statistics`: 그래프 통계 수집 (레이블, 관계, 인덱스 카디널리티)
//! - `cost_model`: 실행 계획 비용 추정
//! - `pattern`: 패턴 매칭 최적화 (시작점 선택, Expand 순서)
//! - `filter`: 필터 푸시다운 최적화 (WHERE 조건 조기 적용)
//! - `union`: UNION 쿼리 최적화 (브랜치 정렬, 병렬 실행, 공통 표현식)

pub mod cost_model;
pub mod filter;
pub mod pattern;
pub mod statistics;
pub mod union;

pub use statistics::{
    IndexStatistics, LabelStatistics, PropertyStatistics, RelationshipStatistics, StatisticsManager,
};

pub use cost_model::CostModel;

pub use pattern::{
    Direction, ExpandPlan, ExpandStep, NodePattern, PathPattern, PathSegment, PatternOptimizer,
    RelationshipPattern,
};

pub use filter::{FilterAnalyzer, FilterPlan, FilterPushdown};
