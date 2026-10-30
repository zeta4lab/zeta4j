//! 캐시 무효화 전략 모듈

/// 무효화 이벤트
#[derive(Debug, Clone)]
pub enum InvalidationEvent {
    /// 스키마 변경 (모든 캐시 무효화)
    SchemaChange,
    /// 인덱스 변경
    IndexChange {
        /// 레이블
        label: String,
        /// 속성
        property: String,
    },
    /// 통계 변경
    StatsChange {
        /// 영향받는 레이블들
        labels: Vec<String>,
    },
    /// 노드 대량 변경
    BulkNodeChange {
        /// 레이블
        label: String,
    },
    /// 관계 대량 변경
    BulkRelationshipChange {
        /// 관계 타입
        rel_type: String,
    },
    /// 수동 무효화
    Manual {
        /// 특정 쿼리 (None이면 전체)
        query: Option<String>,
    },
}

impl InvalidationEvent {
    /// 스키마 변경 이벤트 생성
    pub fn schema_change() -> Self {
        Self::SchemaChange
    }

    /// 인덱스 변경 이벤트 생성
    pub fn index_change(label: impl Into<String>, property: impl Into<String>) -> Self {
        Self::IndexChange {
            label: label.into(),
            property: property.into(),
        }
    }

    /// 통계 변경 이벤트 생성
    pub fn stats_change(labels: Vec<String>) -> Self {
        Self::StatsChange { labels }
    }

    /// 노드 대량 변경 이벤트 생성
    pub fn bulk_node_change(label: impl Into<String>) -> Self {
        Self::BulkNodeChange {
            label: label.into(),
        }
    }

    /// 관계 대량 변경 이벤트 생성
    pub fn bulk_relationship_change(rel_type: impl Into<String>) -> Self {
        Self::BulkRelationshipChange {
            rel_type: rel_type.into(),
        }
    }

    /// 수동 무효화 이벤트 생성
    pub fn manual(query: Option<String>) -> Self {
        Self::Manual { query }
    }

    /// 이벤트 설명
    pub fn description(&self) -> String {
        match self {
            Self::SchemaChange => "Schema change".to_string(),
            Self::IndexChange { label, property } => {
                format!("Index change on {}({})", label, property)
            }
            Self::StatsChange { labels } => {
                format!("Stats change on labels: {:?}", labels)
            }
            Self::BulkNodeChange { label } => {
                format!("Bulk node change on label: {}", label)
            }
            Self::BulkRelationshipChange { rel_type } => {
                format!("Bulk relationship change on type: {}", rel_type)
            }
            Self::Manual { query } => match query {
                Some(q) => format!("Manual invalidation for query: {}", q),
                None => "Manual invalidation (all)".to_string(),
            },
        }
    }

    /// 전체 무효화가 필요한 이벤트인지
    pub fn requires_full_invalidation(&self) -> bool {
        matches!(self, Self::SchemaChange | Self::Manual { query: None })
    }

    /// 관련 레이블 반환
    pub fn affected_labels(&self) -> Vec<String> {
        match self {
            Self::IndexChange { label, .. } => vec![label.clone()],
            Self::StatsChange { labels } => labels.clone(),
            Self::BulkNodeChange { label } => vec![label.clone()],
            _ => vec![],
        }
    }
}

/// 무효화 결과
#[derive(Debug, Clone, Default)]
pub struct InvalidationResult {
    /// 무효화된 파싱 캐시 수
    pub parse_invalidated: usize,
    /// 무효화된 실행 계획 캐시 수
    pub plan_invalidated: usize,
    /// 처리된 이벤트
    pub event_description: String,
}

impl InvalidationResult {
    /// 새 결과 생성
    pub fn new(event: &InvalidationEvent) -> Self {
        Self {
            parse_invalidated: 0,
            plan_invalidated: 0,
            event_description: event.description(),
        }
    }

    /// 총 무효화 수
    pub fn total_invalidated(&self) -> usize {
        self.parse_invalidated + self.plan_invalidated
    }
}
