// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! GraphEngine CRUD 연산
//!
//! ## MVCC 통합 (v1.63.0)
//!
//! - 생성 시: 버전 정보 생성, Serializable에서 쓰기 추적
//! - 읽기 시: 가시성 체크, Serializable에서 읽기 추적
//! - 업데이트/삭제 시: 충돌 감지, 버전 삭제 표시

use indexmap::IndexMap;
use std::time::Instant;

use crate::core::{Edge, EdgeId, Node, NodeId, PropertyContainer, Value};
use crate::error::{GraphError, Result};
use crate::transaction::lock::{LockMode, LockResource};
use crate::transaction::{TransactionId, VersionInfo};

use super::core::{GraphEngine, VersionKey};

impl GraphEngine {
    // ========== 노드 생성 ==========

    /// 노드 생성 - 트랜잭션 지원
    pub async fn create_node(&self, labels: Vec<String>) -> Result<NodeId> {
        let start = Instant::now();

        // Auto-commit 트랜잭션 시작
        let tx_id = self.begin_transaction()?;

        match self.create_node_internal(tx_id, labels).await {
            Ok(node_id) => {
                self.commit_transaction(tx_id)?;

                // Lock-free 통계 업데이트
                self.stats.record_node_operation();
                self.stats.record_query(start.elapsed().as_nanos() as u64);

                Ok(node_id)
            }
            Err(e) => {
                self.abort_transaction(tx_id)?;
                Err(e)
            }
        }
    }

    /// 내부 노드 생성 로직 (트랜잭션 컨텍스트 내에서 실행)
    pub async fn create_node_internal(
        &self,
        tx_id: TransactionId,
        labels: Vec<String>,
    ) -> Result<NodeId> {
        let node_id = self.allocate_node_id()?;
        self.insert_node_internal(tx_id, node_id, labels).await
    }

    /// 지정 ID로 노드 생성 (트랜잭션 컨텍스트 내에서 실행)
    pub async fn create_node_with_id_internal(
        &self,
        tx_id: TransactionId,
        node_id: NodeId,
        labels: Vec<String>,
    ) -> Result<NodeId> {
        self.observe_node_id(node_id)?;
        self.insert_node_internal(tx_id, node_id, labels).await
    }

    /// 공통 노드 삽입 로직
    async fn insert_node_internal(
        &self,
        tx_id: TransactionId,
        node_id: NodeId,
        labels: Vec<String>,
    ) -> Result<NodeId> {
        if self
            .get_node_in_transaction(tx_id, node_id)
            .await?
            .is_some()
        {
            return Err(GraphError::Constraint {
                constraint_name: "node_id".to_string(),
                message: format!("Node ID {node_id} already exists"),
            });
        }

        // 스키마 검증 (Flexible 모드가 아닐 때만)
        {
            let schema_manager = self.schema_manager.read();
            let mode = schema_manager.mode();
            if mode != crate::schema::SchemaMode::Flexible {
                let empty_props = IndexMap::new();
                for label in &labels {
                    schema_manager.validate_node(label, &empty_props)?;
                }
            }
        }

        // 메모리 풀에서 노드 가져오기
        let mut node_guard = self.node_pool.acquire();

        // 노드 초기화
        *node_guard = Node::new(node_id);
        for label in &labels {
            node_guard.add_label(label.clone());
        }
        let node = node_guard.clone();

        // 락 획득 (쓰기 락) - 1초 타임아웃
        self.acquire_transaction_lock(tx_id, LockResource::Node(node_id), LockMode::Exclusive)
            .await?;

        // Admission must succeed before any MVCC/version state is published.
        self.record_node_upsert(tx_id, None, node)?;

        // MVCC: 버전 정보 생성
        let version = VersionInfo::new(tx_id);
        self.version_store
            .write()
            .insert(VersionKey::node(node_id), version);

        // MVCC: Serializable에서 쓰기 추적
        self.mvcc_engine.record_node_write(tx_id, node_id);

        Ok(node_id)
    }

    /// 지정 ID로 노드 생성 - 트랜잭션 지원
    ///
    /// dump/load 시 원래 노드 ID를 보존하기 위해 사용.
    /// 사용 후 반드시 `sync_id_counters()`를 호출하여 ID 카운터를 동기화해야 한다.
    pub async fn create_node_with_id(
        &self,
        node_id: NodeId,
        labels: Vec<String>,
    ) -> Result<NodeId> {
        let start = Instant::now();
        let tx_id = self.begin_transaction()?;

        match self
            .create_node_with_id_internal(tx_id, node_id, labels)
            .await
        {
            Ok(id) => {
                self.commit_transaction(tx_id)?;
                self.stats.record_node_operation();
                self.stats.record_query(start.elapsed().as_nanos() as u64);
                Ok(id)
            }
            Err(e) => {
                self.abort_transaction(tx_id)?;
                Err(e)
            }
        }
    }

    /// 배치 노드 생성 - 최대 10x 성능 향상
    pub async fn batch_create_nodes(&self, labels_list: Vec<Vec<String>>) -> Result<Vec<NodeId>> {
        let start = Instant::now();
        let tx_id = self.begin_transaction()?;
        let mut node_ids = Vec::with_capacity(labels_list.len());
        for labels in labels_list {
            match self.create_node_internal(tx_id, labels).await {
                Ok(node_id) => node_ids.push(node_id),
                Err(error) => {
                    self.abort_transaction(tx_id)?;
                    return Err(error);
                }
            }
        }
        self.commit_transaction(tx_id)?;

        // Lock-free 통계 업데이트
        self.stats.record_batch_operation();
        for _ in 0..node_ids.len() {
            self.stats.record_node_operation();
        }
        self.stats.record_query(start.elapsed().as_nanos() as u64);

        Ok(node_ids)
    }

    // ========== 엣지 생성 ==========

    /// 엣지 생성 - 트랜잭션 지원
    pub async fn create_edge(
        &self,
        relation_type: String,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Result<EdgeId> {
        let start = Instant::now();

        // Auto-commit 트랜잭션 시작
        let tx_id = self.begin_transaction()?;

        match self
            .create_edge_internal(tx_id, relation_type, from_node, to_node)
            .await
        {
            Ok(edge_id) => {
                self.commit_transaction(tx_id)?;

                // Lock-free 통계 업데이트
                self.stats.record_edge_operation();
                self.stats.record_query(start.elapsed().as_nanos() as u64);

                Ok(edge_id)
            }
            Err(e) => {
                self.abort_transaction(tx_id)?;
                Err(e)
            }
        }
    }

    /// 벌크 로드용 엣지 생성 (참조 무결성/스키마 검증 건너뜀)
    ///
    /// dump/load 등 이미 검증된 데이터를 복원할 때 사용.
    pub async fn create_edge_bulk(
        &self,
        relation_type: String,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Result<EdgeId> {
        let start = Instant::now();
        let tx_id = self.begin_transaction()?;
        let edge_id = self.allocate_edge_id()?;

        match self
            .insert_edge_unchecked(tx_id, edge_id, relation_type, from_node, to_node)
            .await
        {
            Ok(edge_id) => {
                self.commit_transaction(tx_id)?;
                self.stats.record_edge_operation();
                self.stats.record_query(start.elapsed().as_nanos() as u64);
                Ok(edge_id)
            }
            Err(e) => {
                self.abort_transaction(tx_id)?;
                Err(e)
            }
        }
    }

    /// 내부 엣지 생성 로직
    pub async fn create_edge_internal(
        &self,
        tx_id: TransactionId,
        relation_type: String,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Result<EdgeId> {
        let edge_id = self.allocate_edge_id()?;
        self.insert_edge_internal(tx_id, edge_id, relation_type, from_node, to_node)
            .await
    }

    /// 지정 ID로 엣지 생성 (트랜잭션 컨텍스트 내에서 실행)
    pub async fn create_edge_with_id_internal(
        &self,
        tx_id: TransactionId,
        edge_id: EdgeId,
        relation_type: String,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Result<EdgeId> {
        self.observe_edge_id(edge_id)?;
        self.insert_edge_internal(tx_id, edge_id, relation_type, from_node, to_node)
            .await
    }

    /// 공통 엣지 삽입 로직 (검증 포함)
    async fn insert_edge_internal(
        &self,
        tx_id: TransactionId,
        edge_id: EdgeId,
        relation_type: String,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Result<EdgeId> {
        self.insert_edge_raw(tx_id, edge_id, relation_type, from_node, to_node, false)
            .await
    }

    /// 벌크 로드용 엣지 삽입 (참조 무결성/스키마 검증 건너뜀)
    async fn insert_edge_unchecked(
        &self,
        tx_id: TransactionId,
        edge_id: EdgeId,
        relation_type: String,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Result<EdgeId> {
        self.insert_edge_raw(tx_id, edge_id, relation_type, from_node, to_node, true)
            .await
    }

    /// 엣지 삽입 공통 구현
    async fn insert_edge_raw(
        &self,
        tx_id: TransactionId,
        edge_id: EdgeId,
        relation_type: String,
        from_node: NodeId,
        to_node: NodeId,
        skip_validation: bool,
    ) -> Result<EdgeId> {
        if self
            .get_edge_in_transaction(tx_id, edge_id)
            .await?
            .is_some()
        {
            return Err(GraphError::Constraint {
                constraint_name: "edge_id".to_string(),
                message: format!("Edge ID {edge_id} already exists"),
            });
        }

        // 노드에 대한 Shared Lock 획득 (삭제 방지)
        self.acquire_transaction_lock(tx_id, LockResource::Node(from_node), LockMode::Shared)
            .await?;
        if from_node != to_node {
            self.acquire_transaction_lock(tx_id, LockResource::Node(to_node), LockMode::Shared)
                .await?;
        }

        if !skip_validation {
            // 참조 무결성 검사
            if self.config.enforce_referential_integrity {
                if self
                    .get_node_in_transaction(tx_id, from_node)
                    .await?
                    .is_none()
                {
                    return Err(GraphError::NodeNotFound { id: from_node });
                }
                if self
                    .get_node_in_transaction(tx_id, to_node)
                    .await?
                    .is_none()
                {
                    return Err(GraphError::NodeNotFound { id: to_node });
                }

                // MVCC: 노드 가시성 체크
                if !self.check_node_visibility(from_node, tx_id)? {
                    return Err(GraphError::NodeNotFound { id: from_node });
                }
                if !self.check_node_visibility(to_node, tx_id)? {
                    return Err(GraphError::NodeNotFound { id: to_node });
                }
            }

            // 스키마 검증 (Flexible 모드가 아닐 때만)
            {
                let mode = self.schema_manager.read().mode();
                if mode != crate::schema::SchemaMode::Flexible {
                    let source_label = self
                        .get_node_in_transaction(tx_id, from_node)
                        .await?
                        .and_then(|n| n.labels().first().cloned());
                    let target_label = self
                        .get_node_in_transaction(tx_id, to_node)
                        .await?
                        .and_then(|n| n.labels().first().cloned());

                    let empty_props = IndexMap::new();
                    let schema_manager = self.schema_manager.read();
                    schema_manager.validate_relationship(
                        &relation_type,
                        &empty_props,
                        source_label.as_deref(),
                        target_label.as_deref(),
                    )?;
                }
            }
        }

        // 메모리 풀에서 엣지 가져오기
        let mut edge_guard = self.edge_pool.acquire();

        // 엣지 초기화
        *edge_guard = Edge::new(edge_id, relation_type.clone(), from_node, to_node);
        let edge = edge_guard.clone();

        // 엣지 ID에 대한 Exclusive Lock 획득
        self.acquire_transaction_lock(tx_id, LockResource::Edge(edge_id), LockMode::Exclusive)
            .await?;

        // Admission must succeed before any MVCC/version state is published.
        self.record_edge_upsert(tx_id, None, edge)?;

        // MVCC: 버전 정보 생성
        let version = VersionInfo::new(tx_id);
        self.version_store
            .write()
            .insert(VersionKey::edge(edge_id), version);

        // MVCC: Serializable에서 쓰기 추적
        self.mvcc_engine.record_edge_write(tx_id, edge_id);

        Ok(edge_id)
    }

    // ========== 조회 ==========

    /// 노드 조회 (Lock-free 통계, MVCC 가시성 검사 포함)
    pub async fn get_node(&self, node_id: NodeId) -> Result<Option<Node>> {
        let start = Instant::now();

        let result = self.committed_nodes.read().get(&node_id).cloned();

        // Lock-free 통계 업데이트
        self.stats.record_query(start.elapsed().as_nanos() as u64);

        Ok(result)
    }

    /// 엣지 조회 (Lock-free 통계, MVCC 가시성 검사 포함)
    pub async fn get_edge(&self, edge_id: EdgeId) -> Result<Option<Edge>> {
        let start = Instant::now();

        let result = self.committed_edges.read().get(&edge_id).cloned();

        // Lock-free 통계 업데이트
        self.stats.record_query(start.elapsed().as_nanos() as u64);

        Ok(result)
    }

    /// 라벨로 canonical committed 노드 찾기.
    pub async fn find_nodes_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
        let start = Instant::now();

        let mut graph_result: Vec<_> = self
            .committed_nodes
            .read()
            .values()
            .filter(|node| node.labels().contains(label))
            .map(Node::id)
            .collect();
        graph_result.sort_unstable();

        // Lock-free 통계 업데이트
        self.stats.record_query(start.elapsed().as_nanos() as u64);

        Ok(graph_result)
    }

    /// 노드와 연결된 모든 엣지 찾기 (양방향, Lock-free 통계)
    pub fn find_edges_by_node(&self, node_id: NodeId) -> Result<Vec<EdgeId>> {
        let start = Instant::now();
        let mut edge_ids: Vec<_> = self
            .committed_edges
            .read()
            .values()
            .filter(|edge| edge.connects_to(node_id))
            .map(Edge::id)
            .collect();
        edge_ids.sort_unstable();
        // Lock-free 통계 업데이트
        self.stats.record_query(start.elapsed().as_nanos() as u64);

        Ok(edge_ids)
    }

    /// 관계 타입과 endpoint로 canonical committed 엣지 찾기.
    pub fn find_edges_by_relationship(
        &self,
        relation_type: &str,
        from_node: Option<NodeId>,
        to_node: Option<NodeId>,
    ) -> Result<Vec<EdgeId>> {
        let start = Instant::now();

        let candidate_ids: Vec<EdgeId> = match (from_node, to_node) {
            (Some(from), _) => self
                .committed_edges
                .read()
                .values()
                .filter(|edge| edge.connects_to(from))
                .map(Edge::id)
                .collect(),
            (_, Some(to)) => self
                .committed_edges
                .read()
                .values()
                .filter(|edge| edge.connects_to(to))
                .map(Edge::id)
                .collect(),
            (None, None) => self.committed_edges.read().keys().copied().collect(),
        };
        let mut result = Vec::new();
        for edge_id in candidate_ids {
            let Some(edge) = self.committed_edges.read().get(&edge_id).cloned() else {
                continue;
            };
            if edge.relation_type() == relation_type
                && from_node.is_none_or(|from| edge.from_node() == from)
                && to_node.is_none_or(|to| edge.to_node() == to)
            {
                result.push(edge_id);
            }
        }

        // Lock-free 통계 업데이트
        self.stats.record_query(start.elapsed().as_nanos() as u64);

        Ok(result)
    }

    // ========== 업데이트 ==========

    /// 노드 업데이트 - 트랜잭션 지원
    pub async fn update_node<F>(&self, node_id: NodeId, update_fn: F) -> Result<()>
    where
        F: FnOnce(&mut Node),
    {
        let start = Instant::now();

        // Auto-commit 트랜잭션
        let tx_id = self.begin_transaction()?;

        match self.update_node_internal(tx_id, node_id, update_fn).await {
            Ok(_) => {
                self.commit_transaction(tx_id)?;

                // Lock-free 통계 업데이트
                self.stats.record_node_operation();
                self.stats.record_query(start.elapsed().as_nanos() as u64);

                Ok(())
            }
            Err(e) => {
                self.abort_transaction(tx_id)?;
                Err(e)
            }
        }
    }

    /// 내부 노드 업데이트 로직
    pub async fn update_node_internal<F>(
        &self,
        tx_id: TransactionId,
        node_id: NodeId,
        update_fn: F,
    ) -> Result<()>
    where
        F: FnOnce(&mut Node),
    {
        // 1. Lock (Exclusive) - 1초 타임아웃
        self.acquire_transaction_lock(tx_id, LockResource::Node(node_id), LockMode::Exclusive)
            .await?;

        // MVCC: 쓰기 충돌 감지
        if let Err(conflict) = self.check_node_write_conflict(node_id, tx_id) {
            return Err(GraphError::Concurrency {
                message: format!("Write conflict on node {}: {}", node_id, conflict),
            });
        }

        // 2. Read current state
        let mut node = self
            .get_node_in_transaction(tx_id, node_id)
            .await?
            .ok_or(GraphError::NodeNotFound { id: node_id })?;

        // MVCC: 가시성 체크 (실패 시 재시도 가능한 Concurrency 에러)
        if !self.check_node_visibility(node_id, tx_id)? {
            return Err(GraphError::Concurrency {
                message: format!("Concurrent modification on node {}", node_id),
            });
        }

        let old_node = node.clone();

        // 3. Update memory copy
        update_fn(&mut node);
        let new_node = node;

        // Admission must succeed before any MVCC/version state is published.
        self.record_node_upsert(tx_id, Some(old_node), new_node)?;

        // MVCC: 버전 삭제 표시 (기존 버전은 삭제된 것으로 표시하고 새 버전 생성)
        {
            let mut versions = self.version_store.write();
            let key = VersionKey::node(node_id);
            if let Some(version) = versions.get_mut(&key) {
                version.mark_deleted(tx_id);
            }
            // 새 버전 정보 생성
            versions.insert(key, VersionInfo::new(tx_id));
        }

        // MVCC: Serializable에서 쓰기 추적
        self.mvcc_engine.record_node_write(tx_id, node_id);

        Ok(())
    }

    /// 엣지 업데이트 (프로퍼티 변경 등)
    pub async fn update_edge<F>(&self, edge_id: EdgeId, update_fn: F) -> Result<()>
    where
        F: FnOnce(&mut Edge),
    {
        let start = Instant::now();

        // Auto-commit 트랜잭션
        let tx_id = self.begin_transaction()?;

        match self.update_edge_internal(tx_id, edge_id, update_fn).await {
            Ok(_) => {
                self.commit_transaction(tx_id)?;

                // Lock-free 통계 업데이트
                self.stats.record_edge_operation();
                self.stats.record_query(start.elapsed().as_nanos() as u64);

                Ok(())
            }
            Err(e) => {
                self.abort_transaction(tx_id)?;
                Err(e)
            }
        }
    }

    /// 내부 엣지 업데이트 로직
    pub async fn update_edge_internal<F>(
        &self,
        tx_id: TransactionId,
        edge_id: EdgeId,
        update_fn: F,
    ) -> Result<()>
    where
        F: FnOnce(&mut Edge),
    {
        // 1. Lock (Exclusive)
        self.acquire_transaction_lock(tx_id, LockResource::Edge(edge_id), LockMode::Exclusive)
            .await?;

        // MVCC: 쓰기 충돌 감지
        if let Err(conflict) = self.check_edge_write_conflict(edge_id, tx_id) {
            return Err(GraphError::Concurrency {
                message: format!("Write conflict on edge {}: {}", edge_id, conflict),
            });
        }

        // 2. Read current state
        let mut edge = self
            .get_edge_in_transaction(tx_id, edge_id)
            .await?
            .ok_or(GraphError::EdgeNotFound { id: edge_id })?;

        // MVCC: 가시성 체크 (실패 시 재시도 가능한 Concurrency 에러)
        if !self.check_edge_visibility(edge_id, tx_id)? {
            return Err(GraphError::Concurrency {
                message: format!("Concurrent modification on edge {}", edge_id),
            });
        }

        let old_edge = edge.clone();

        // 3. Update memory copy
        update_fn(&mut edge);
        let new_edge = edge;

        // Admission must succeed before any MVCC/version state is published.
        self.record_edge_upsert(tx_id, Some(old_edge), new_edge)?;

        // MVCC: 버전 삭제 표시 및 새 버전 생성
        {
            let mut versions = self.version_store.write();
            let key = VersionKey::edge(edge_id);
            if let Some(version) = versions.get_mut(&key) {
                version.mark_deleted(tx_id);
            }
            versions.insert(key, VersionInfo::new(tx_id));
        }

        // MVCC: Serializable에서 쓰기 추적
        self.mvcc_engine.record_edge_write(tx_id, edge_id);

        Ok(())
    }

    // ========== 삭제 ==========

    /// 노드 삭제 - 트랜잭션 지원
    pub async fn delete_node(&self, node_id: NodeId) -> Result<()> {
        let start = Instant::now();
        let tx_id = self.begin_transaction()?;

        match self.delete_node_internal(tx_id, node_id, false).await {
            Ok(_) => {
                self.commit_transaction(tx_id)?;

                // Lock-free 통계 업데이트
                self.stats.record_node_operation();
                self.stats.record_query(start.elapsed().as_nanos() as u64);

                Ok(())
            }
            Err(e) => {
                self.abort_transaction(tx_id)?;
                Err(e)
            }
        }
    }

    /// DETACH DELETE - 연결된 엣지 포함 삭제
    pub async fn detach_delete_node(&self, node_id: NodeId) -> Result<()> {
        let start = Instant::now();
        let tx_id = self.begin_transaction()?;

        match self.delete_node_internal(tx_id, node_id, true).await {
            Ok(_) => {
                self.commit_transaction(tx_id)?;

                // Lock-free 통계 업데이트
                self.stats.record_node_operation();
                self.stats.record_query(start.elapsed().as_nanos() as u64);

                Ok(())
            }
            Err(e) => {
                self.abort_transaction(tx_id)?;
                Err(e)
            }
        }
    }

    /// 내부 노드 삭제 로직
    pub async fn delete_node_internal(
        &self,
        tx_id: TransactionId,
        node_id: NodeId,
        detach: bool,
    ) -> Result<()> {
        // 1. 락 획득
        self.acquire_transaction_lock(tx_id, LockResource::Node(node_id), LockMode::Exclusive)
            .await?;

        // MVCC: 쓰기 충돌 감지
        if let Err(conflict) = self.check_node_write_conflict(node_id, tx_id) {
            return Err(GraphError::Concurrency {
                message: format!("Write conflict on node {}: {}", node_id, conflict),
            });
        }

        // 2. 노드 존재 확인
        let node = self
            .get_node_in_transaction(tx_id, node_id)
            .await?
            .ok_or(GraphError::NodeNotFound { id: node_id })?;

        // MVCC: 가시성 체크 (실패 시 재시도 가능한 Concurrency 에러)
        if !self.check_node_visibility(node_id, tx_id)? {
            return Err(GraphError::Concurrency {
                message: format!("Concurrent modification on node {}", node_id),
            });
        }

        // 3. 연결된 엣지 확인
        let connected_edges = self
            .get_connected_edges_in_transaction(tx_id, node_id)
            .await?;

        if !connected_edges.is_empty() {
            if detach {
                // DETACH DELETE: 연결된 엣지 먼저 삭제
                for edge_id in connected_edges {
                    self.delete_edge_internal(tx_id, edge_id).await?;
                }
            } else {
                // 일반 DELETE: 연결된 엣지가 있으면 에러
                let edge_count = connected_edges.len();
                return Err(GraphError::Constraint {
                    constraint_name: "integrity".to_string(),
                    message: format!(
                        "Cannot delete node {} with {} connected edges. Use DETACH DELETE.",
                        node_id, edge_count
                    ),
                });
            }
        }

        // Admission must succeed before any MVCC/version state is published.
        self.record_node_delete(tx_id, node)?;

        // MVCC: 버전 삭제 표시
        {
            let mut versions = self.version_store.write();
            let key = VersionKey::node(node_id);
            if let Some(version) = versions.get_mut(&key) {
                version.mark_deleted(tx_id);
            }
        }

        // MVCC: Serializable에서 쓰기 추적
        self.mvcc_engine.record_node_write(tx_id, node_id);

        Ok(())
    }

    /// 엣지 삭제 - 트랜잭션 지원
    pub async fn delete_edge(&self, edge_id: EdgeId) -> Result<()> {
        let start = Instant::now();
        let tx_id = self.begin_transaction()?;

        match self.delete_edge_internal(tx_id, edge_id).await {
            Ok(_) => {
                self.commit_transaction(tx_id)?;

                // Lock-free 통계 업데이트
                self.stats.record_edge_operation();
                self.stats.record_query(start.elapsed().as_nanos() as u64);

                Ok(())
            }
            Err(e) => {
                self.abort_transaction(tx_id)?;
                Err(e)
            }
        }
    }

    /// 내부 엣지 삭제 로직
    pub async fn delete_edge_internal(&self, tx_id: TransactionId, edge_id: EdgeId) -> Result<()> {
        // 1. 락 획득
        self.acquire_transaction_lock(tx_id, LockResource::Edge(edge_id), LockMode::Exclusive)
            .await?;

        // MVCC: 쓰기 충돌 감지
        if let Err(conflict) = self.check_edge_write_conflict(edge_id, tx_id) {
            return Err(GraphError::Concurrency {
                message: format!("Write conflict on edge {}: {}", edge_id, conflict),
            });
        }

        // 2. 엣지 조회
        let edge = self
            .get_edge_in_transaction(tx_id, edge_id)
            .await?
            .ok_or(GraphError::EdgeNotFound { id: edge_id })?;

        // MVCC: 가시성 체크 (실패 시 재시도 가능한 Concurrency 에러)
        if !self.check_edge_visibility(edge_id, tx_id)? {
            return Err(GraphError::Concurrency {
                message: format!("Concurrent modification on edge {}", edge_id),
            });
        }

        // Admission must succeed before any MVCC/version state is published.
        self.record_edge_delete(tx_id, edge)?;

        // MVCC: 버전 삭제 표시
        {
            let mut versions = self.version_store.write();
            let key = VersionKey::edge(edge_id);
            if let Some(version) = versions.get_mut(&key) {
                version.mark_deleted(tx_id);
            }
        }

        // MVCC: Serializable에서 쓰기 추적
        self.mvcc_engine.record_edge_write(tx_id, edge_id);

        Ok(())
    }

    // ========== 벡터 연산 (Issue #91) ==========

    /// 노드에 벡터 프로퍼티 설정
    ///
    /// # Arguments
    /// * `node_id` - 대상 노드 ID
    /// * `property` - 벡터를 저장할 프로퍼티 이름
    /// * `vector` - 벡터 데이터
    ///
    /// # Example
    /// ```ignore
    /// engine.set_node_vector(node_id, "embedding", vec![0.1, 0.2, 0.3]).await?;
    /// ```
    pub async fn set_node_vector(
        &self,
        node_id: NodeId,
        property: &str,
        vector: Vec<f32>,
    ) -> Result<()> {
        use crate::core::VectorValue;

        let vector_value = VectorValue::new(vector).map_err(|e| GraphError::Validation {
            message: format!("Invalid vector: {}", e),
        })?;

        let prop_name = property.to_string();
        self.update_node(node_id, move |node| {
            node.set_property(&prop_name, Value::Vector(vector_value));
        })
        .await
    }

    /// 노드에서 벡터 프로퍼티 조회
    ///
    /// # Arguments
    /// * `node_id` - 대상 노드 ID
    /// * `property` - 벡터 프로퍼티 이름
    ///
    /// # Returns
    /// * `Ok(Some(Vec<f32>))` - 벡터 데이터
    /// * `Ok(None)` - 프로퍼티가 없거나 벡터가 아닌 경우
    /// * `Err` - 노드가 없는 경우
    ///
    /// # Example
    /// ```ignore
    /// let embedding = engine.get_node_vector(node_id, "embedding").await?;
    /// ```
    pub async fn get_node_vector(
        &self,
        node_id: NodeId,
        property: &str,
    ) -> Result<Option<Vec<f32>>> {
        let node = self.get_node(node_id).await?;

        match node {
            Some(node) => match node.get_property(property) {
                Some(Value::Vector(v)) => Ok(Some(v.as_slice().to_vec())),
                _ => Ok(None),
            },
            None => Err(GraphError::NodeNotFound { id: node_id }),
        }
    }
}
