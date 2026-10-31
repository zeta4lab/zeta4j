// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 엣지 스토리지 타입 정의

use crate::core::{Edge, EdgeId, NodeId};
use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// Instant을 직렬화 가능하게 하는 모듈
pub(crate) mod instant_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{Duration, Instant};

    pub fn serialize<S>(instant: &Instant, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = instant.elapsed();
        duration.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Instant, D::Error>
    where
        D: Deserializer<'de>,
    {
        let duration = Duration::deserialize(deserializer)?;
        Ok(Instant::now() - duration)
    }
}

/// EdgeStorage 스냅샷 (직렬화 가능)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeStorageSnapshot {
    pub edges: HashMap<EdgeId, Edge>,
    pub outgoing_edges: HashMap<NodeId, IndexSet<EdgeId>>,
    pub incoming_edges: HashMap<NodeId, IndexSet<EdgeId>>,
    pub type_index: HashMap<String, IndexSet<EdgeId>>,
    pub next_edge_id: u64,
    pub timestamp_nanos: u64,
}

/// 압축 통계 (엣지 스토리지용)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeCompactionStats {
    pub before_size: usize,
    pub after_size: usize,
    pub reclaimed_bytes: usize,
    pub compaction_time: Duration,
    #[serde(with = "instant_serde")]
    pub completed_at: Instant,
}

/// 스토리지 트랜잭션 스냅샷
#[derive(Debug, Clone)]
pub struct EdgeTransactionSnapshot {
    pub edges: HashMap<EdgeId, Edge>,
    pub outgoing_edges: HashMap<NodeId, IndexSet<EdgeId>>,
    pub incoming_edges: HashMap<NodeId, IndexSet<EdgeId>>,
    pub type_index: HashMap<String, IndexSet<EdgeId>>,
    pub next_edge_id: u64,
    pub created_at: Instant,
}

/// 엣지 스토리지 통계
#[derive(Debug, Clone)]
pub struct EdgeStorageStats {
    pub total_edges: usize,
    pub memory_usage: usize,
    pub insert_count: u64,
    pub lookup_count: u64,
    pub update_count: u64,
    pub delete_count: u64,
}

/// 관계 패턴 (패턴 매칭용)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RelationshipPattern {
    /// 특정 관계 타입
    Type(String),
    /// 특정 소스-타겟 노드 쌍
    SourceTarget(NodeId, NodeId),
    /// 경로 탐색 (시작 노드, 관계 타입들, 최대 깊이)
    Path {
        start: NodeId,
        types: Vec<String>,
        depth: usize,
    },
}
