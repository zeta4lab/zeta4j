//! 패턴 매칭 및 경로 순회

use crate::core::{EdgeId, NodeId};
use crate::error::Result;
use std::collections::{HashSet, VecDeque};

use super::EdgeStorage;
use super::types::RelationshipPattern;

impl EdgeStorage {
    /// 관계 패턴 매칭
    pub async fn match_pattern(&self, pattern: RelationshipPattern) -> Result<Vec<EdgeId>> {
        match pattern {
            RelationshipPattern::Type(edge_type) => {
                let disk = self.disk.read().await;
                Ok(disk.get_edges_by_type(&edge_type).await)
            }
            RelationshipPattern::SourceTarget(source, target) => {
                let disk = self.disk.read().await;
                Ok(disk.get_edges_between(source, target).await)
            }
            RelationshipPattern::Path {
                start,
                types,
                depth,
            } => self.traverse_path(start, types, depth).await,
        }
    }

    /// 경로 탐색 (BFS/DFS)
    pub async fn traverse_path(
        &self,
        start: NodeId,
        edge_types: Vec<String>,
        max_depth: usize,
    ) -> Result<Vec<EdgeId>> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        queue.push_back((start, 0));

        let disk = self.disk.read().await;

        while let Some((node_id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            if !visited.insert(node_id) {
                continue;
            }

            // 노드의 모든 나가는 엣지 확인
            let edge_ids = disk.get_outgoing_edges(node_id).await;
            for edge_id in edge_ids {
                if let Ok(Some(edge)) = disk.get(edge_id).await
                    && (edge_types.is_empty()
                        || edge_types.contains(&edge.relation_type().to_string()))
                {
                    result.push(edge_id);
                    queue.push_back((edge.to_node(), depth + 1));
                }
            }
        }

        Ok(result)
    }

    /// 관계 강도 계산
    pub async fn calculate_relationship_strength(&self, source: NodeId, target: NodeId) -> f64 {
        let edges_count = self.get_edges_between(source, target).await.len();

        // 단순 예: 엣지 수를 강도로 사용
        edges_count as f64
    }
}
