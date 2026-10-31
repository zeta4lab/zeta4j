// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 스냅샷 및 체크포인트

use crate::error::{GraphError, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::EdgeStorage;
use super::types::{EdgeCompactionStats, EdgeStorageSnapshot, EdgeTransactionSnapshot};
use crate::storage::graph::common::InternalElementStats;

impl EdgeStorage {
    /// 설계 문서용 스냅샷 생성
    pub async fn create_design_snapshot(&self) -> EdgeTransactionSnapshot {
        // scan_all()로 모든 엣지를 읽어서 스냅샷 생성
        let disk = self.disk.read().await;
        let all_edges = disk.scan_all().await.unwrap_or_default();

        let mut edges_map = HashMap::new();
        let mut outgoing = HashMap::new();
        let mut incoming = HashMap::new();
        let mut type_idx = HashMap::new();

        for edge in all_edges {
            let edge_id = edge.id();
            edges_map.insert(edge_id, edge.clone());

            outgoing
                .entry(edge.from_node())
                .or_insert_with(indexmap::IndexSet::new)
                .insert(edge_id);

            incoming
                .entry(edge.to_node())
                .or_insert_with(indexmap::IndexSet::new)
                .insert(edge_id);

            type_idx
                .entry(edge.relation_type().to_string())
                .or_insert_with(indexmap::IndexSet::new)
                .insert(edge_id);
        }

        EdgeTransactionSnapshot {
            edges: edges_map,
            outgoing_edges: outgoing,
            incoming_edges: incoming,
            type_index: type_idx,
            next_edge_id: self.next_edge_id.load(Ordering::SeqCst),
            created_at: Instant::now(),
        }
    }

    /// 메모리 압축 (설계 문서용)
    pub async fn compact_design(&self) -> EdgeCompactionStats {
        let start_time = Instant::now();
        let before_size = self.memory_usage().await;

        self.compact().await;

        let after_size = self.memory_usage().await;
        let reclaimed = before_size.saturating_sub(after_size);

        EdgeCompactionStats {
            before_size,
            after_size,
            reclaimed_bytes: reclaimed,
            compaction_time: start_time.elapsed(),
            completed_at: Instant::now(),
        }
    }

    /// 스냅샷 생성
    pub async fn create_snapshot(&self) -> EdgeStorageSnapshot {
        // scan_all()로 모든 엣지를 읽어서 스냅샷 생성
        let disk = self.disk.read().await;
        let all_edges = disk.scan_all().await.unwrap_or_default();

        let mut edges_map = HashMap::new();
        let mut outgoing = HashMap::new();
        let mut incoming = HashMap::new();
        let mut type_idx = HashMap::new();

        for edge in all_edges {
            let edge_id = edge.id();
            edges_map.insert(edge_id, edge.clone());

            outgoing
                .entry(edge.from_node())
                .or_insert_with(indexmap::IndexSet::new)
                .insert(edge_id);

            incoming
                .entry(edge.to_node())
                .or_insert_with(indexmap::IndexSet::new)
                .insert(edge_id);

            type_idx
                .entry(edge.relation_type().to_string())
                .or_insert_with(indexmap::IndexSet::new)
                .insert(edge_id);
        }

        EdgeStorageSnapshot {
            edges: edges_map,
            outgoing_edges: outgoing,
            incoming_edges: incoming,
            type_index: type_idx,
            next_edge_id: self.next_edge_id.load(Ordering::SeqCst),
            timestamp_nanos: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        }
    }

    /// 체크포인트 생성 (비동기)
    pub async fn checkpoint(&self, path: &Path) -> Result<()> {
        let snapshot = self.create_snapshot().await;

        let serialized =
            zeta4j_codec::serialize(&snapshot).map_err(|e| GraphError::Serialization {
                message: format!("EdgeStorage checkpoint serialization failed: {}", e),
                source: None,
            })?;

        let mut file = File::create(path).await?;
        file.write_all(&serialized).await?;
        file.flush().await?;

        Ok(())
    }

    /// 체크포인트에서 복구 (비동기)
    pub async fn restore_from_checkpoint(&self, path: &Path) -> Result<()> {
        let mut file = File::open(path).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;

        let snapshot: EdgeStorageSnapshot =
            zeta4j_codec::deserialize(&buffer).map_err(|e| GraphError::Serialization {
                message: format!("EdgeStorage checkpoint deserialization failed: {}", e),
                source: None,
            })?;

        // clear 후 스냅샷의 모든 엣지를 삽입
        {
            let mut disk = self.disk.write().await;
            disk.clear().await?;

            for (_edge_id, edge) in snapshot.edges {
                disk.insert(&edge).await?;
            }
        }

        // ID 카운터 복원
        self.next_edge_id
            .store(snapshot.next_edge_id, Ordering::SeqCst);

        // 통계 재계산
        {
            let disk = self.disk.read().await;
            let mut stats = self.stats.write().await;
            let total_elements = disk.scan_all().await?.len();
            *stats = InternalElementStats {
                total_elements,
                ..Default::default()
            };
        }

        Ok(())
    }
}
