//! 배치 연산

use crate::core::{Edge, EdgeId};
use crate::error::{GraphError, Result};

use super::EdgeStorage;

impl EdgeStorage {
    /// 배치 엣지 삽입 (트랜잭션 지원)
    pub async fn batch_insert_design(&self, edges: Vec<Edge>) -> Result<usize> {
        match self.batch_insert(edges.clone()).await {
            Ok(()) => Ok(edges.len()),
            Err(e) => Err(e),
        }
    }

    /// 배치 엣지 삽입
    pub async fn batch_insert(&self, edges: Vec<Edge>) -> Result<()> {
        let mut disk = self.disk.write().await;
        let mut stats = self.stats.write().await;

        for edge in edges {
            let memory_size = edge.memory_size();
            disk.insert(&edge).await?;

            stats.total_elements += 1;
            stats.memory_usage += memory_size;
            stats.insert_count += 1;
        }

        Ok(())
    }

    /// 배치 엣지 업데이트
    pub async fn batch_update<F>(&self, edge_ids: &[EdgeId], mut update_fn: F) -> Result<usize>
    where
        F: FnMut(&mut Edge) -> bool,
    {
        let mut disk = self.disk.write().await;
        let mut stats = self.stats.write().await;
        let mut updated_count = 0;

        for &edge_id in edge_ids {
            if let Some(mut edge) = disk.get(edge_id).await? {
                let old_memory = edge.memory_size();
                if update_fn(&mut edge) {
                    disk.update(&edge).await?;
                    let new_memory = edge.memory_size();
                    stats.memory_usage = stats.memory_usage + new_memory - old_memory;
                    stats.update_count += 1;
                    updated_count += 1;
                }
            }
        }

        Ok(updated_count)
    }

    /// 배치 엣지 삭제
    pub async fn batch_delete(&self, edge_ids: &[EdgeId]) -> Result<Vec<Edge>> {
        let mut deleted_edges = Vec::new();

        for &edge_id in edge_ids {
            match self.remove(edge_id).await {
                Ok(edge) => deleted_edges.push(edge),
                Err(GraphError::EdgeNotFound { .. }) => continue, // 이미 삭제된 엣지는 무시
                Err(e) => return Err(e),
            }
        }

        Ok(deleted_edges)
    }

    /// 조건부 엣지 삭제
    pub async fn delete_edges_where<F>(&self, mut condition: F) -> Result<Vec<EdgeId>>
    where
        F: FnMut(&Edge) -> bool,
    {
        let mut deleted_ids = Vec::new();

        // 먼저 삭제할 엣지들 수집
        {
            let disk = self.disk.read().await;
            if let Ok(all_edges) = disk.scan_all().await {
                for edge in all_edges {
                    if condition(&edge) {
                        deleted_ids.push(edge.id());
                    }
                }
            }
        }

        // 수집된 엣지들 삭제
        for edge_id in &deleted_ids {
            self.remove(*edge_id).await?;
        }

        Ok(deleted_ids)
    }
}
