//! GraphEngine 트랜잭션 관리 메서드
//!
//! ## MVCC 통합 (v1.63.0)
//!
//! - `begin_transaction_with_isolation`: 격리 수준 지정 트랜잭션 시작
//! - MVCC 스냅샷 캡처 및 가시성 체크
//! - SSI (Serializable Snapshot Isolation) 검증
//!
use std::collections::{BTreeSet, HashMap};

use parking_lot::{ArcMutexGuard, RawMutex};

use crate::core::{Edge, Node};
use crate::error::{GraphError, Result};
use crate::index::CompositeIndexManager;
use crate::storage::memory::{MemoryCategory, MemoryReservation};
use crate::transaction::lock::{LockMode, LockResource};
use crate::transaction::manager::PreparedTransactionCommit;
use crate::transaction::{
    ConflictError, IsolationLevel, TransactionError, TransactionId, TransactionState,
};

use super::core::{GraphEngine, VersionKey};

/// Final canonical entity state produced by one active graph transaction.
///
/// The delta is captured before commit. Upserts contain the final entity value;
/// deletes identify entities absent after the transaction. It contains no page,
/// slot, WAL, or cache identity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphTransactionDelta {
    pub upsert_nodes: Vec<Node>,
    pub delete_node_ids: Vec<u64>,
    pub upsert_edges: Vec<Edge>,
    pub delete_edge_ids: Vec<u64>,
}

/// Single-use proof that one active graph transaction has been frozen and
/// exclusively claimed by its commit coordinator.
pub struct PreparedGraphCommit {
    tx_id: TransactionId,
    delta: GraphTransactionDelta,
    transaction: PreparedTransactionCommit,
    composite_projection: CompositeIndexManager,
    committed_memory_before: usize,
    committed_memory_after: usize,
    _prepared_memory: Option<MemoryReservation>,
    _prepared_index_memory: Option<MemoryReservation>,
    _index_definition_guard: ArcMutexGuard<RawMutex, ()>,
}

impl std::fmt::Debug for PreparedGraphCommit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedGraphCommit")
            .field("tx_id", &self.tx_id)
            .field("delta", &self.delta)
            .finish_non_exhaustive()
    }
}

impl PreparedGraphCommit {
    pub const fn transaction_id(&self) -> TransactionId {
        self.tx_id
    }

    pub const fn delta(&self) -> &GraphTransactionDelta {
        &self.delta
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GraphTransactionOverlay {
    original_nodes: HashMap<u64, Option<Node>>,
    final_nodes: HashMap<u64, Option<Node>>,
    original_edges: HashMap<u64, Option<Edge>>,
    final_edges: HashMap<u64, Option<Edge>>,
}

#[derive(Debug, Clone)]
enum OverlayEntity<T> {
    Untouched,
    Present(T),
    Deleted,
}

impl GraphTransactionOverlay {
    const NODE_ENTRY_OVERHEAD: usize = std::mem::size_of::<(u64, Option<Node>)>();
    const EDGE_ENTRY_OVERHEAD: usize = std::mem::size_of::<(u64, Option<Edge>)>();

    fn memory_size(&self) -> usize {
        let original_nodes = self
            .original_nodes
            .values()
            .map(|node| {
                Self::NODE_ENTRY_OVERHEAD.saturating_add(node.as_ref().map_or(0, Node::memory_size))
            })
            .fold(0usize, usize::saturating_add);
        let final_nodes = self
            .final_nodes
            .values()
            .map(|node| {
                Self::NODE_ENTRY_OVERHEAD.saturating_add(node.as_ref().map_or(0, Node::memory_size))
            })
            .fold(0usize, usize::saturating_add);
        let original_edges = self
            .original_edges
            .values()
            .map(|edge| {
                Self::EDGE_ENTRY_OVERHEAD.saturating_add(edge.as_ref().map_or(0, Edge::memory_size))
            })
            .fold(0usize, usize::saturating_add);
        let final_edges = self
            .final_edges
            .values()
            .map(|edge| {
                Self::EDGE_ENTRY_OVERHEAD.saturating_add(edge.as_ref().map_or(0, Edge::memory_size))
            })
            .fold(0usize, usize::saturating_add);
        std::mem::size_of::<Self>()
            .saturating_add(original_nodes)
            .saturating_add(final_nodes)
            .saturating_add(original_edges)
            .saturating_add(final_edges)
    }

    fn node(&self, node_id: u64) -> OverlayEntity<Node> {
        match self.final_nodes.get(&node_id) {
            Some(Some(node)) => OverlayEntity::Present(node.clone()),
            Some(None) => OverlayEntity::Deleted,
            None => OverlayEntity::Untouched,
        }
    }

    fn edge(&self, edge_id: u64) -> OverlayEntity<Edge> {
        match self.final_edges.get(&edge_id) {
            Some(Some(edge)) => OverlayEntity::Present(edge.clone()),
            Some(None) => OverlayEntity::Deleted,
            None => OverlayEntity::Untouched,
        }
    }

    fn upsert_node(&mut self, original: Option<Node>, node: Node) {
        self.original_nodes.entry(node.id()).or_insert(original);
        self.final_nodes.insert(node.id(), Some(node));
    }

    fn delete_node(&mut self, original: Node) {
        self.original_nodes
            .entry(original.id())
            .or_insert_with(|| Some(original.clone()));
        self.final_nodes.insert(original.id(), None);
    }

    fn upsert_edge(&mut self, original: Option<Edge>, edge: Edge) {
        self.original_edges.entry(edge.id()).or_insert(original);
        self.final_edges.insert(edge.id(), Some(edge));
    }

    fn delete_edge(&mut self, original: Edge) {
        self.original_edges
            .entry(original.id())
            .or_insert_with(|| Some(original.clone()));
        self.final_edges.insert(original.id(), None);
    }

    fn freeze(self) -> GraphTransactionDelta {
        let mut delta = GraphTransactionDelta::default();
        for (id, final_node) in self.final_nodes {
            let original = self.original_nodes.get(&id).cloned().flatten();
            if original == final_node {
                continue;
            }
            match final_node {
                Some(node) => delta.upsert_nodes.push(node),
                None if original.is_some() => delta.delete_node_ids.push(id),
                None => {}
            }
        }
        for (id, final_edge) in self.final_edges {
            let original = self.original_edges.get(&id).cloned().flatten();
            if original == final_edge {
                continue;
            }
            match final_edge {
                Some(edge) => delta.upsert_edges.push(edge),
                None if original.is_some() => delta.delete_edge_ids.push(id),
                None => {}
            }
        }
        delta.upsert_nodes.sort_unstable_by_key(Node::id);
        delta.delete_node_ids.sort_unstable();
        delta.upsert_edges.sort_unstable_by_key(Edge::id);
        delta.delete_edge_ids.sort_unstable();
        delta
    }
}

impl GraphTransactionDelta {
    pub fn is_empty(&self) -> bool {
        self.upsert_nodes.is_empty()
            && self.delete_node_ids.is_empty()
            && self.upsert_edges.is_empty()
            && self.delete_edge_ids.is_empty()
    }

    fn memory_size(&self) -> usize {
        let node_bytes = self
            .upsert_nodes
            .iter()
            .map(Node::memory_size)
            .fold(0usize, usize::saturating_add);
        let edge_bytes = self
            .upsert_edges
            .iter()
            .map(Edge::memory_size)
            .fold(0usize, usize::saturating_add);
        std::mem::size_of::<Self>()
            .saturating_add(node_bytes)
            .saturating_add(edge_bytes)
            .saturating_add(
                self.delete_node_ids
                    .capacity()
                    .saturating_mul(std::mem::size_of::<u64>()),
            )
            .saturating_add(
                self.delete_edge_ids
                    .capacity()
                    .saturating_mul(std::mem::size_of::<u64>()),
            )
    }
}

impl GraphEngine {
    fn committed_memory_after(&self, delta: &GraphTransactionDelta) -> usize {
        let changed_nodes: BTreeSet<_> = delta
            .delete_node_ids
            .iter()
            .copied()
            .chain(delta.upsert_nodes.iter().map(Node::id))
            .collect();
        let changed_edges: BTreeSet<_> = delta
            .delete_edge_ids
            .iter()
            .copied()
            .chain(delta.upsert_edges.iter().map(Edge::id))
            .collect();
        let retained_nodes = self
            .committed_nodes
            .read()
            .values()
            .filter(|node| !changed_nodes.contains(&node.id()))
            .map(Node::memory_size)
            .fold(0usize, usize::saturating_add);
        let retained_edges = self
            .committed_edges
            .read()
            .values()
            .filter(|edge| !changed_edges.contains(&edge.id()))
            .map(Edge::memory_size)
            .fold(0usize, usize::saturating_add);
        retained_nodes
            .saturating_add(retained_edges)
            .saturating_add(
                delta
                    .upsert_nodes
                    .iter()
                    .map(Node::memory_size)
                    .fold(0usize, usize::saturating_add),
            )
            .saturating_add(
                delta
                    .upsert_edges
                    .iter()
                    .map(Edge::memory_size)
                    .fold(0usize, usize::saturating_add),
            )
    }

    // --- 트랜잭션 관리 메서드 ---

    /// 트랜잭션 시작 (기본 격리 수준: ReadCommitted)
    pub fn begin_transaction(&self) -> Result<TransactionId> {
        self.begin_transaction_with_isolation(IsolationLevel::ReadCommitted)
    }

    /// 격리 수준을 지정하여 트랜잭션 시작
    ///
    /// # 격리 수준
    /// - `ReadCommitted`: 각 읽기마다 최신 커밋된 데이터 조회
    /// - `RepeatableRead`: 트랜잭션 시작 시점의 스냅샷 고정
    /// - `Serializable`: SSI 기반 충돌 감지 (rw-antidependency 검사)
    ///
    /// # Example
    /// ```ignore
    /// let tx_id = engine.begin_transaction_with_isolation(IsolationLevel::Serializable)?;
    /// // ... 연산 수행
    /// engine.commit_transaction(tx_id)?;
    /// ```
    pub fn begin_transaction_with_isolation(
        &self,
        isolation_level: IsolationLevel,
    ) -> Result<TransactionId> {
        let overlay = GraphTransactionOverlay::default();
        let reservation = if let Some(limiter) = &self.memory_limiter {
            Some(limiter.try_reserve(MemoryCategory::TransactionOverlay, overlay.memory_size())?)
        } else {
            None
        };
        let tx_id = self.transaction_manager.begin_transaction()?;

        // MVCC 스냅샷 캡처
        self.mvcc_engine
            .begin_transaction_with_isolation(tx_id, isolation_level);

        // 격리 수준 저장
        self.tx_isolation_levels
            .write()
            .insert(tx_id, isolation_level);
        self.transaction_overlays.write().insert(tx_id, overlay);
        if let Some(reservation) = reservation {
            self.transaction_memory.lock().insert(tx_id, reservation);
        }

        Ok(tx_id)
    }

    /// 트랜잭션의 격리 수준 조회
    pub fn get_isolation_level(&self, tx_id: TransactionId) -> Option<IsolationLevel> {
        self.tx_isolation_levels.read().get(&tx_id).copied()
    }

    /// Freeze the final canonical entity state of an active transaction.
    ///
    /// This method does not commit, flush, or mutate transaction state. Callers
    /// may validate and apply the returned delta to an external coordinator and
    /// still roll the graph transaction back if preparation fails.
    pub async fn prepare_transaction_delta(
        &self,
        tx_id: TransactionId,
    ) -> Result<GraphTransactionDelta> {
        if self.transaction_manager.get_transaction_state(tx_id)? != TransactionState::Active {
            return Err(GraphError::System {
                message: format!("cannot prepare inactive graph transaction {tx_id}"),
            });
        }

        self.freeze_transaction_delta(tx_id)
            .map(|(delta, _reservation)| delta)
    }

    fn freeze_transaction_delta(
        &self,
        tx_id: TransactionId,
    ) -> Result<(GraphTransactionDelta, Option<MemoryReservation>)> {
        let overlays = self.transaction_overlays.read();
        let overlay = overlays.get(&tx_id).ok_or_else(|| GraphError::System {
            message: format!("graph transaction overlay {tx_id} is missing"),
        })?;
        // Reserve the cloned overlay and materialized delta before either
        // allocation starts. Completion can also clone both the old and new
        // node projections, so retain enough headroom for the larger of that
        // overlay-backed working set and the canonical persistence delta clone.
        let overlay_bytes = overlay.memory_size();
        let clone_headroom = overlay_bytes.saturating_mul(4);
        let mut reservation =
            self.try_reserve_memory(MemoryCategory::TransactionOverlay, clone_headroom)?;
        let cloned = overlay.clone();
        drop(overlays);
        let delta = cloned.freeze();
        let delta_bytes = delta.memory_size();
        let prepared_bytes = delta_bytes.saturating_add(overlay_bytes.max(delta_bytes));
        if let Some(reservation) = reservation.as_mut() {
            reservation.resize(prepared_bytes)?;
        }
        Ok((delta, reservation))
    }

    /// Freeze and exclusively claim a transaction for an external canonical
    /// commit. The returned token must be completed or aborted exactly once.
    pub fn prepare_transaction_commit(&self, tx_id: TransactionId) -> Result<PreparedGraphCommit> {
        if self.transaction_manager.get_transaction_state(tx_id)? != TransactionState::Active {
            return Err(GraphError::System {
                message: format!("cannot prepare inactive graph transaction {tx_id}"),
            });
        }

        let isolation_level = self.tx_isolation_levels.read().get(&tx_id).copied();
        if isolation_level == Some(IsolationLevel::Serializable)
            && let Err(ConflictError::SerializationFailure) =
                self.mvcc_engine.validate_serializable(tx_id)
        {
            self.rollback_transaction_internal(tx_id)?;
            return Err(GraphError::System {
                message: "Transaction aborted: Serialization failure - concurrent transaction conflict detected".into(),
            });
        }

        let transaction = self.transaction_manager.begin_commit(tx_id)?;
        let preparation = (|| {
            let (delta, prepared_memory) = self.freeze_transaction_delta(tx_id)?;
            let index_definition_guard = self.index_definition_lock.lock_arc();
            let index_multiplier = self
                .canonical_store
                .index_definitions()?
                .into_iter()
                .filter(|definition| {
                    !matches!(
                        definition.kind,
                        super::GraphIndexKind::Fulltext { .. }
                            | super::GraphIndexKind::Vector { .. }
                    )
                })
                .count();
            let changed_node_bytes = delta
                .upsert_nodes
                .iter()
                .map(Node::memory_size)
                .fold(0usize, usize::saturating_add)
                .saturating_mul(index_multiplier)
                .saturating_mul(4);
            let current_composite_bytes = self.composite_index_manager.read().memory_size();
            let index_clone_headroom = current_composite_bytes.saturating_add(changed_node_bytes);
            let index_clone_headroom = if index_clone_headroom == 0 {
                0
            } else {
                index_clone_headroom.saturating_add(4096)
            };
            let mut prepared_index_memory =
                self.try_reserve_memory(MemoryCategory::Index, index_clone_headroom)?;
            let composite_projection = self.prepare_composite_index_projection(&delta)?;
            let index_headroom = composite_projection
                .memory_size()
                .saturating_add(changed_node_bytes);
            if let Some(reservation) = prepared_index_memory.as_mut() {
                reservation.resize(index_headroom)?;
            }
            let committed_memory_before = self
                .committed_memory
                .lock()
                .as_ref()
                .map_or(0, MemoryReservation::bytes);
            let committed_memory_after = self.committed_memory_after(&delta);
            if committed_memory_after > committed_memory_before
                && let Some(reservation) = self.committed_memory.lock().as_mut()
            {
                reservation.resize(committed_memory_after)?;
            }
            Ok((
                delta,
                composite_projection,
                committed_memory_before,
                committed_memory_after,
                prepared_memory,
                prepared_index_memory,
                index_definition_guard,
            ))
        })();

        match preparation {
            Ok((
                delta,
                composite_projection,
                committed_memory_before,
                committed_memory_after,
                prepared_memory,
                prepared_index_memory,
                index_definition_guard,
            )) => Ok(PreparedGraphCommit {
                tx_id,
                delta,
                transaction,
                composite_projection,
                committed_memory_before,
                committed_memory_after,
                _prepared_memory: prepared_memory,
                _prepared_index_memory: prepared_index_memory,
                _index_definition_guard: index_definition_guard,
            }),
            Err(error) => {
                self.transaction_manager.abort_commit(transaction);
                self.rollback_transaction_resources(tx_id);
                Err(error)
            }
        }
    }

    fn transaction_overlay(&self, tx_id: TransactionId) -> Result<GraphTransactionOverlay> {
        if self.transaction_manager.get_transaction_state(tx_id)? != TransactionState::Active {
            return Err(GraphError::System {
                message: format!("cannot read through inactive graph transaction {tx_id}"),
            });
        }
        self.transaction_overlays
            .read()
            .get(&tx_id)
            .cloned()
            .ok_or_else(|| GraphError::System {
                message: format!("graph transaction overlay {tx_id} is missing"),
            })
    }

    /// Return the transaction-local final node state, falling back to the
    /// committed store only when this transaction has not touched the node.
    pub async fn get_node_in_transaction(
        &self,
        tx_id: TransactionId,
        node_id: u64,
    ) -> Result<Option<Node>> {
        match self.transaction_overlay(tx_id)?.node(node_id) {
            OverlayEntity::Present(node) => Ok(Some(node)),
            OverlayEntity::Deleted => Ok(None),
            OverlayEntity::Untouched => Ok(self.committed_nodes.read().get(&node_id).cloned()),
        }
    }

    /// Return the transaction-local final edge state, falling back to the
    /// committed store only when this transaction has not touched the edge.
    pub async fn get_edge_in_transaction(
        &self,
        tx_id: TransactionId,
        edge_id: u64,
    ) -> Result<Option<Edge>> {
        match self.transaction_overlay(tx_id)?.edge(edge_id) {
            OverlayEntity::Present(edge) => Ok(Some(edge)),
            OverlayEntity::Deleted => Ok(None),
            OverlayEntity::Untouched => Ok(self.committed_edges.read().get(&edge_id).cloned()),
        }
    }

    /// Resolve a label lookup against the transaction's final node view.
    pub async fn find_nodes_by_label_in_transaction(
        &self,
        tx_id: TransactionId,
        label: &str,
    ) -> Result<Vec<u64>> {
        let overlay = self.transaction_overlay(tx_id)?;
        let mut node_ids: BTreeSet<u64> = self
            .committed_nodes
            .read()
            .values()
            .filter(|node| node.labels().contains(label))
            .map(Node::id)
            .collect();
        for (node_id, final_node) in &overlay.final_nodes {
            node_ids.remove(node_id);
            if final_node
                .as_ref()
                .is_some_and(|node| node.labels().contains(label))
            {
                node_ids.insert(*node_id);
            }
        }
        Ok(node_ids.into_iter().collect())
    }

    /// Resolve adjacency against the transaction's final edge view.
    pub async fn get_connected_edges_in_transaction(
        &self,
        tx_id: TransactionId,
        node_id: u64,
    ) -> Result<Vec<u64>> {
        let overlay = self.transaction_overlay(tx_id)?;
        let mut edge_ids: BTreeSet<u64> = self
            .committed_edges
            .read()
            .values()
            .filter(|edge| edge.connects_to(node_id))
            .map(Edge::id)
            .collect();
        for (edge_id, final_edge) in &overlay.final_edges {
            edge_ids.remove(edge_id);
            if final_edge
                .as_ref()
                .is_some_and(|edge| edge.connects_to(node_id))
            {
                edge_ids.insert(*edge_id);
            }
        }
        Ok(edge_ids.into_iter().collect())
    }

    /// Return every node identity visible in the transaction's final view.
    pub async fn all_node_ids_in_transaction(&self, tx_id: TransactionId) -> Result<Vec<u64>> {
        let overlay = self.transaction_overlay(tx_id)?;
        let mut node_ids: BTreeSet<u64> = self.committed_nodes.read().keys().copied().collect();
        for (node_id, final_node) in &overlay.final_nodes {
            if final_node.is_some() {
                node_ids.insert(*node_id);
            } else {
                node_ids.remove(node_id);
            }
        }
        Ok(node_ids.into_iter().collect())
    }

    /// Return every edge identity visible in the transaction's final view.
    pub async fn all_edge_ids_in_transaction(&self, tx_id: TransactionId) -> Result<Vec<u64>> {
        let overlay = self.transaction_overlay(tx_id)?;
        let mut edge_ids: BTreeSet<u64> = self.committed_edges.read().keys().copied().collect();
        for (edge_id, final_edge) in &overlay.final_edges {
            if final_edge.is_some() {
                edge_ids.insert(*edge_id);
            } else {
                edge_ids.remove(edge_id);
            }
        }
        Ok(edge_ids.into_iter().collect())
    }

    pub(crate) fn record_node_upsert(
        &self,
        tx_id: TransactionId,
        original: Option<Node>,
        node: Node,
    ) -> Result<()> {
        let headroom = GraphTransactionOverlay::NODE_ENTRY_OVERHEAD
            .saturating_mul(2)
            .saturating_add(original.as_ref().map_or(0, Node::memory_size))
            .saturating_add(node.memory_size());
        self.update_transaction_overlay(tx_id, headroom, move |overlay| {
            overlay.upsert_node(original, node);
        })
    }

    pub(crate) fn record_node_delete(&self, tx_id: TransactionId, node: Node) -> Result<()> {
        let headroom = GraphTransactionOverlay::NODE_ENTRY_OVERHEAD
            .saturating_mul(2)
            .saturating_add(node.memory_size());
        self.update_transaction_overlay(tx_id, headroom, move |overlay| {
            overlay.delete_node(node);
        })
    }

    pub(crate) fn record_edge_upsert(
        &self,
        tx_id: TransactionId,
        original: Option<Edge>,
        edge: Edge,
    ) -> Result<()> {
        let headroom = GraphTransactionOverlay::EDGE_ENTRY_OVERHEAD
            .saturating_mul(2)
            .saturating_add(original.as_ref().map_or(0, Edge::memory_size))
            .saturating_add(edge.memory_size());
        self.update_transaction_overlay(tx_id, headroom, move |overlay| {
            overlay.upsert_edge(original, edge);
        })
    }

    pub(crate) fn record_edge_delete(&self, tx_id: TransactionId, edge: Edge) -> Result<()> {
        let headroom = GraphTransactionOverlay::EDGE_ENTRY_OVERHEAD
            .saturating_mul(2)
            .saturating_add(edge.memory_size());
        self.update_transaction_overlay(tx_id, headroom, move |overlay| {
            overlay.delete_edge(edge);
        })
    }

    fn update_transaction_overlay(
        &self,
        tx_id: TransactionId,
        mutation_headroom: usize,
        update: impl FnOnce(&mut GraphTransactionOverlay),
    ) -> Result<()> {
        let mut overlays = self.transaction_overlays.write();
        let overlay = overlays.get_mut(&tx_id).ok_or_else(|| GraphError::System {
            message: format!("graph transaction overlay {tx_id} is missing"),
        })?;
        let staging_headroom = overlay.memory_size().saturating_add(mutation_headroom);
        let mut staged_reservation =
            self.try_reserve_memory(MemoryCategory::TransactionOverlay, staging_headroom)?;
        let mut staged = overlay.clone();
        update(&mut staged);
        let staged_bytes = staged.memory_size();
        if let Some(reservation) = staged_reservation.as_mut() {
            reservation.resize(staged_bytes)?;
        }

        if let Some(staged_reservation) = staged_reservation {
            let mut transaction_memory = self.transaction_memory.lock();
            let owner = transaction_memory
                .get_mut(&tx_id)
                .ok_or_else(|| GraphError::System {
                    message: format!("graph transaction memory owner {tx_id} is missing"),
                })?;
            *overlay = staged;
            let previous = std::mem::replace(owner, staged_reservation);
            drop(previous);
        } else {
            *overlay = staged;
        }
        Ok(())
    }

    pub(crate) fn commit_index_memory(&self, prepared: &mut Option<MemoryReservation>) {
        let measured = self.measured_index_memory_usage();
        let mut index_memory = self.index_memory.lock();
        let Some(permanent) = index_memory.as_mut() else {
            return;
        };
        if measured > permanent.bytes() {
            let additional = measured - permanent.bytes();
            let Some(prepared) = prepared.as_mut() else {
                tracing::error!(
                    measured,
                    current = permanent.bytes(),
                    "index projection grew without prepared memory"
                );
                return;
            };
            match prepared.split(additional) {
                Ok(growth) => {
                    if let Err(error) = permanent.merge(growth) {
                        tracing::error!(%error, "failed to transfer prepared index memory");
                    }
                }
                Err(error) => {
                    tracing::error!(
                        %error,
                        measured,
                        current = permanent.bytes(),
                        prepared = prepared.bytes(),
                        "prepared index headroom was insufficient"
                    );
                }
            }
        } else if measured < permanent.bytes()
            && let Err(error) = permanent.resize(measured)
        {
            tracing::error!(%error, "failed to shrink index memory reservation");
        }
    }

    /// 트랜잭션 커밋
    ///
    /// # Serializable 격리 수준
    /// Serializable 격리 수준에서는 커밋 전 SSI 검증을 수행합니다.
    /// rw-antidependency 사이클이 감지되면 `SerializationFailure` 에러가 발생합니다.
    pub fn commit_transaction(&self, tx_id: TransactionId) -> Result<()> {
        let prepared = self.prepare_transaction_commit(tx_id)?;
        if let Err(commit_error) = self.canonical_store.commit_graph(prepared.delta().clone()) {
            self.abort_transaction_commit(prepared);
            return Err(commit_error);
        }
        self.complete_transaction_commit(prepared);
        Ok(())
    }

    /// Finalize a successfully persisted canonical commit. Every fallible
    /// projection operation was completed while creating the prepared token.
    pub fn complete_transaction_commit(&self, prepared: PreparedGraphCommit) {
        let PreparedGraphCommit {
            tx_id,
            delta,
            transaction,
            composite_projection,
            committed_memory_before: _,
            committed_memory_after,
            _prepared_memory: prepared_memory,
            _prepared_index_memory: mut prepared_index_memory,
            _index_definition_guard: index_definition_guard,
        } = prepared;
        let GraphTransactionDelta {
            upsert_nodes,
            delete_node_ids,
            upsert_edges,
            delete_edge_ids,
        } = delta;
        let mut changed_node_ids: BTreeSet<_> = delete_node_ids.iter().copied().collect();
        changed_node_ids.extend(upsert_nodes.iter().map(Node::id));
        let old_nodes = {
            let nodes = self.committed_nodes.read();
            changed_node_ids
                .iter()
                .filter_map(|node_id| nodes.get(node_id).cloned())
                .collect::<Vec<_>>()
        };
        let new_nodes = upsert_nodes.clone();
        {
            let mut nodes = self.committed_nodes.write();
            for node_id in delete_node_ids {
                nodes.remove(&node_id);
            }
            for node in upsert_nodes {
                nodes.insert(node.id(), node);
            }
        }
        {
            let mut edges = self.committed_edges.write();
            for edge_id in delete_edge_ids {
                edges.remove(&edge_id);
            }
            for edge in upsert_edges {
                edges.insert(edge.id(), edge);
            }
        }
        self.publish_node_index_projection_delta(&old_nodes, &new_nodes);
        *self.composite_index_manager.write() = composite_projection;
        self.commit_index_memory(&mut prepared_index_memory);
        if let Some(reservation) = self.committed_memory.lock().as_mut()
            && reservation.bytes() != committed_memory_after
            && let Err(error) = reservation.resize(committed_memory_after)
        {
            tracing::error!(%error, "committed memory reservation shrink failed");
        }
        let commit_ts = self.mvcc_engine.commit_transaction(tx_id);
        self.finalize_versions(tx_id, commit_ts);
        self.transaction_manager.complete_commit(transaction);
        self.transaction_overlays.write().remove(&tx_id);
        self.transaction_memory.lock().remove(&tx_id);
        self.tx_isolation_levels.write().remove(&tx_id);
        self.lock_manager.release_all_locks(tx_id);
        drop(prepared_memory);
        drop(prepared_index_memory);
        drop(index_definition_guard);
    }

    /// Abort a prepared transaction after its canonical persistence failed.
    pub fn abort_transaction_commit(&self, prepared: PreparedGraphCommit) {
        let tx_id = prepared.transaction.transaction_id();
        if let Some(reservation) = self.committed_memory.lock().as_mut()
            && let Err(error) = reservation.resize(prepared.committed_memory_before)
        {
            tracing::error!(%error, "failed to restore committed memory reservation after abort");
        }
        self.transaction_manager.abort_commit(prepared.transaction);
        self.rollback_transaction_resources(tx_id);
    }

    /// 트랜잭션 롤백
    pub fn rollback_transaction(&self, tx_id: TransactionId) -> Result<()> {
        self.rollback_transaction_internal(tx_id)
    }

    /// Abort a transaction and clean every engine-owned resource.
    ///
    /// Unlike the user-facing strict rollback operation, this cleanup primitive
    /// is idempotent. Deadlock detection and protocol recovery can therefore
    /// converge on the same terminal state without replacing the original
    /// failure with `TransactionNotFound`.
    pub fn abort_transaction(&self, tx_id: TransactionId) -> Result<()> {
        self.transaction_manager.abort_if_active(tx_id)?;
        self.rollback_transaction_resources(tx_id);
        Ok(())
    }

    /// Acquire a transaction lock and immediately abort a selected deadlock
    /// victim before returning the canonical deadlock error to its caller.
    pub(crate) async fn acquire_transaction_lock(
        &self,
        tx_id: TransactionId,
        resource: LockResource,
        mode: LockMode,
    ) -> Result<()> {
        match self
            .lock_manager
            .acquire_lock_with_timeout(tx_id, resource, mode, self.config.lock_timeout_ms)
            .await
        {
            Ok(()) => Ok(()),
            Err(error @ TransactionError::DeadlockDetected(_)) => {
                self.abort_transaction(tx_id)?;
                Err(error.into())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Canonical writes occur only after prepare, so rollback only discards
    /// transaction-local state and releases concurrency-control resources.
    fn rollback_transaction_internal(&self, tx_id: TransactionId) -> Result<()> {
        // Claim the terminal state before releasing transaction resources so
        // a prepared commit cannot be rolled back by an unrelated caller.
        self.transaction_manager.rollback_transaction(tx_id)?;
        self.rollback_transaction_resources(tx_id);
        Ok(())
    }

    fn rollback_transaction_resources(&self, tx_id: TransactionId) {
        self.lock_manager.release_all_locks(tx_id);
        self.mvcc_engine.rollback_transaction(tx_id);
        self.cleanup_uncommitted_versions(tx_id);
        self.tx_isolation_levels.write().remove(&tx_id);
        self.transaction_overlays.write().remove(&tx_id);
        self.transaction_memory.lock().remove(&tx_id);
    }

    /// 커밋된 트랜잭션의 버전에 타임스탬프 설정
    fn finalize_versions(&self, tx_id: TransactionId, commit_ts: u64) {
        let mut versions = self.version_store.write();
        for version in versions.values_mut() {
            if version.created_by == tx_id && version.created_ts == 0 {
                version.set_commit_timestamp(commit_ts);
            }
            if version.deleted_by == Some(tx_id) && version.deleted_ts.is_none() {
                version.set_delete_timestamp(commit_ts);
            }
        }
    }

    /// 롤백된 트랜잭션의 미커밋 버전 정리
    fn cleanup_uncommitted_versions(&self, tx_id: TransactionId) {
        let mut versions = self.version_store.write();

        // 해당 트랜잭션이 생성한 버전 제거
        versions.retain(|_, v| v.created_by != tx_id || v.created_ts != 0);

        // 해당 트랜잭션이 삭제 표시한 버전의 삭제 표시 취소
        for version in versions.values_mut() {
            if version.deleted_by == Some(tx_id) && version.deleted_ts.is_none() {
                version.deleted_by = None;
            }
        }
    }

    // ========== MVCC 헬퍼 메서드 ==========

    /// 노드의 MVCC 가시성 체크
    ///
    /// 주어진 트랜잭션에서 해당 노드가 보이는지 확인합니다.
    pub fn check_node_visibility(&self, node_id: u64, tx_id: TransactionId) -> Result<bool> {
        let key = VersionKey::node(node_id);
        let versions = self.version_store.read();

        match versions.get(&key) {
            Some(version) => Ok(self.mvcc_engine.check_visibility(version, tx_id)?),
            None => Ok(true), // 버전 정보가 없으면 항상 보임 (레거시 데이터)
        }
    }

    /// 엣지의 MVCC 가시성 체크
    pub fn check_edge_visibility(&self, edge_id: u64, tx_id: TransactionId) -> Result<bool> {
        let key = VersionKey::edge(edge_id);
        let versions = self.version_store.read();

        match versions.get(&key) {
            Some(version) => Ok(self.mvcc_engine.check_visibility(version, tx_id)?),
            None => Ok(true), // 버전 정보가 없으면 항상 보임
        }
    }

    /// 노드 쓰기 충돌 감지
    ///
    /// 다른 트랜잭션이 이미 해당 노드를 수정했는지 확인합니다.
    pub fn check_node_write_conflict(
        &self,
        node_id: u64,
        tx_id: TransactionId,
    ) -> std::result::Result<(), ConflictError> {
        let key = VersionKey::node(node_id);
        let versions = self.version_store.read();

        match versions.get(&key) {
            Some(version) => self.mvcc_engine.check_write_conflict(version, tx_id),
            None => Ok(()),
        }
    }

    /// 엣지 쓰기 충돌 감지
    pub fn check_edge_write_conflict(
        &self,
        edge_id: u64,
        tx_id: TransactionId,
    ) -> std::result::Result<(), ConflictError> {
        let key = VersionKey::edge(edge_id);
        let versions = self.version_store.read();

        match versions.get(&key) {
            Some(version) => self.mvcc_engine.check_write_conflict(version, tx_id),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::core::{PropertyContainer, Value};
    use crate::engine::{GraphEngine, GraphEngineConfig, InMemoryCanonicalGraphStore};
    use crate::error::GraphError;
    use crate::storage::memory::{MemoryCategory, MemoryLimiterConfig};
    use crate::transaction::TransactionError;
    use crate::transaction::lock::{LockMode, LockResource};

    fn category_current(engine: &GraphEngine, category: MemoryCategory) -> usize {
        engine
            .memory_limiter_stats()
            .and_then(|stats| {
                stats
                    .category(category)
                    .map(|category| category.current_usage)
            })
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn transaction_memory_moves_to_committed_and_rolls_back_without_leak()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = GraphEngineConfig::default().with_memory_limit(MemoryLimiterConfig {
            max_memory_bytes: 1024 * 1024,
            auto_cleanup_enabled: false,
            ..MemoryLimiterConfig::default()
        });
        let engine =
            GraphEngine::new(config, Arc::new(InMemoryCanonicalGraphStore::default())).await?;

        let rollback_tx = engine.begin_transaction()?;
        engine
            .create_node_internal(rollback_tx, vec!["Rollback".to_string()])
            .await?;
        assert!(category_current(&engine, MemoryCategory::TransactionOverlay) > 0);
        assert_eq!(engine.reconcile_memory_usage(), Some(0));
        engine.rollback_transaction(rollback_tx)?;
        assert_eq!(
            category_current(&engine, MemoryCategory::TransactionOverlay),
            0
        );
        assert_eq!(category_current(&engine, MemoryCategory::CommittedGraph), 0);

        let commit_tx = engine.begin_transaction()?;
        engine
            .create_node_internal(commit_tx, vec!["Committed".to_string()])
            .await?;
        engine.commit_transaction(commit_tx)?;
        assert_eq!(
            category_current(&engine, MemoryCategory::TransactionOverlay),
            0
        );
        assert!(category_current(&engine, MemoryCategory::CommittedGraph) > 0);
        assert_eq!(engine.reconcile_memory_usage(), Some(0));
        Ok(())
    }

    #[tokio::test]
    async fn rejected_overlay_growth_does_not_mutate_overlay_or_projection()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = GraphEngineConfig::default().with_memory_limit(MemoryLimiterConfig {
            max_memory_bytes: 512,
            auto_cleanup_enabled: false,
            ..MemoryLimiterConfig::default()
        });
        let engine =
            GraphEngine::new(config, Arc::new(InMemoryCanonicalGraphStore::default())).await?;
        let tx_id = engine
            .begin_transaction_with_isolation(crate::transaction::IsolationLevel::Serializable)?;
        let result = engine
            .create_node_internal(tx_id, vec!["far-too-large-for-the-budget".repeat(32)])
            .await;
        assert!(matches!(result, Err(GraphError::Memory(_))));
        assert!(engine.all_node_ids_in_transaction(tx_id).await?.is_empty());
        assert_eq!(engine.node_count().await?, 0);
        assert!(engine.version_store.read().is_empty());
        assert!(engine.mvcc_engine.get_write_set(tx_id).is_empty());
        engine.rollback_transaction(tx_id)?;
        assert_eq!(
            category_current(&engine, MemoryCategory::TransactionOverlay),
            0
        );
        Ok(())
    }

    #[tokio::test]
    async fn rejected_transaction_baseline_leaves_no_active_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = GraphEngineConfig::default().with_memory_limit(MemoryLimiterConfig {
            max_memory_bytes: 1,
            auto_cleanup_enabled: false,
            ..MemoryLimiterConfig::default()
        });
        let engine =
            GraphEngine::new(config, Arc::new(InMemoryCanonicalGraphStore::default())).await?;

        assert!(matches!(
            engine.begin_transaction(),
            Err(GraphError::Memory(_))
        ));
        assert_eq!(engine.transaction_manager.active_transaction_count(), 0);
        assert!(engine.transaction_overlays.read().is_empty());
        assert!(engine.transaction_memory.lock().is_empty());
        let transaction_stats = engine.transaction_manager.stats();
        assert_eq!(transaction_stats.active, 0);
        assert_eq!(transaction_stats.aborted, 0);
        assert_eq!(
            engine
                .memory_limiter_stats()
                .map(|stats| stats.current_usage),
            Some(0)
        );
        Ok(())
    }

    #[tokio::test]
    async fn canonical_engine_creates_no_page_files_and_advances_explicit_ids()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir()?;
        let config = GraphEngineConfig {
            data_path: temp_dir.path().to_string_lossy().into_owned(),
            ..GraphEngineConfig::default()
        };
        let engine =
            GraphEngine::new(config, Arc::new(InMemoryCanonicalGraphStore::default())).await?;

        let explicit_tx = engine.begin_transaction()?;
        engine
            .create_node_with_id_internal(explicit_tx, 41, vec!["Explicit".to_string()])
            .await?;
        engine.commit_transaction(explicit_tx)?;
        assert_eq!(engine.create_node(vec!["Automatic".to_string()]).await?, 42);
        assert_eq!(
            engine.get_all_labels(),
            vec!["Explicit".to_string(), "Automatic".to_string()]
        );

        let mut entries = std::fs::read_dir(temp_dir.path())?;
        assert!(entries.next().is_none());
        Ok(())
    }

    #[tokio::test]
    async fn transaction_delta_freezes_final_canonical_entities()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = GraphEngine::with_temp_dir().await?;
        let create_tx = engine.begin_transaction()?;
        let first = engine
            .create_node_internal(create_tx, vec!["Person".to_string()])
            .await?;
        let second = engine
            .create_node_internal(create_tx, vec!["Company".to_string()])
            .await?;
        let edge = engine
            .create_edge_internal(create_tx, "WORKS_AT".to_string(), first, second)
            .await?;
        let transient = engine
            .create_node_internal(create_tx, vec!["Transient".to_string()])
            .await?;
        engine
            .delete_node_internal(create_tx, transient, false)
            .await?;
        engine
            .update_node_internal(create_tx, first, |node| {
                node.set_property("name", Value::String("Ada".to_string()));
            })
            .await?;

        let created = engine.prepare_transaction_delta(create_tx).await?;
        assert_eq!(created.upsert_nodes.len(), 2);
        assert_eq!(created.upsert_edges.len(), 1);
        assert!(created.delete_node_ids.is_empty());
        assert!(created.delete_edge_ids.is_empty());
        assert_eq!(created.upsert_nodes[0].id(), first);
        assert_eq!(
            created.upsert_nodes[0].get_property("name"),
            Some(&Value::String("Ada".to_string()))
        );
        engine.commit_transaction(create_tx)?;

        let delete_tx = engine.begin_transaction()?;
        engine
            .update_node_internal(delete_tx, first, |node| {
                node.set_property("name", Value::String("Ada".to_string()));
            })
            .await?;
        engine.delete_edge_internal(delete_tx, edge).await?;
        engine
            .delete_node_internal(delete_tx, second, false)
            .await?;
        let deleted = engine.prepare_transaction_delta(delete_tx).await?;
        assert_eq!(deleted.delete_node_ids, vec![second]);
        assert_eq!(deleted.delete_edge_ids, vec![edge]);
        assert!(deleted.upsert_nodes.is_empty());
        assert!(deleted.upsert_edges.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn transaction_reads_resolve_final_overlay_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = GraphEngine::with_temp_dir().await?;
        let tx_id = engine.begin_transaction()?;
        let person = engine
            .create_node_internal(tx_id, vec!["Person".to_string()])
            .await?;
        let company = engine
            .create_node_internal(tx_id, vec!["Company".to_string()])
            .await?;
        let edge = engine
            .create_edge_internal(tx_id, "WORKS_AT".to_string(), person, company)
            .await?;

        engine
            .update_node_internal(tx_id, person, |node| {
                node.add_label("Researcher".to_string());
                node.set_property("name", Value::String("Ada".to_string()));
            })
            .await?;

        let visible_person = engine
            .get_node_in_transaction(tx_id, person)
            .await?
            .ok_or_else(|| std::io::Error::other("person must be visible in its transaction"))?;
        assert!(visible_person.labels().contains("Researcher"));
        assert_eq!(
            visible_person.get_property("name"),
            Some(&Value::String("Ada".to_string()))
        );
        assert_eq!(
            engine
                .find_nodes_by_label_in_transaction(tx_id, "Researcher")
                .await?,
            vec![person]
        );
        assert_eq!(
            engine
                .get_connected_edges_in_transaction(tx_id, person)
                .await?,
            vec![edge]
        );

        engine.delete_edge_internal(tx_id, edge).await?;
        engine.delete_node_internal(tx_id, company, false).await?;
        assert!(engine.get_edge_in_transaction(tx_id, edge).await?.is_none());
        assert!(
            engine
                .get_node_in_transaction(tx_id, company)
                .await?
                .is_none()
        );
        assert!(
            engine
                .get_connected_edges_in_transaction(tx_id, person)
                .await?
                .is_empty()
        );
        assert_eq!(
            engine.all_node_ids_in_transaction(tx_id).await?,
            vec![person]
        );
        assert!(engine.all_edge_ids_in_transaction(tx_id).await?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn prepared_commit_exclusively_owns_completion_or_abort()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = GraphEngine::with_temp_dir().await?;
        let aborted_tx = engine.begin_transaction()?;
        let aborted_node = engine
            .create_node_internal(aborted_tx, vec!["Aborted".to_string()])
            .await?;
        let aborted = engine.prepare_transaction_commit(aborted_tx)?;
        assert_eq!(aborted.transaction_id(), aborted_tx);
        assert_eq!(aborted.delta().upsert_nodes.len(), 1);
        assert_eq!(
            engine
                .transaction_manager
                .get_transaction_state(aborted_tx)?,
            crate::transaction::TransactionState::Committing
        );
        assert!(engine.rollback_transaction(aborted_tx).is_err());
        engine.abort_transaction_commit(aborted);
        assert!(
            engine
                .transaction_manager
                .get_transaction_context(aborted_tx)
                .is_none()
        );
        assert!(engine.get_node(aborted_node).await?.is_none());

        let committed_tx = engine.begin_transaction()?;
        let committed_node = engine
            .create_node_internal(committed_tx, vec!["Committed".to_string()])
            .await?;
        let committed = engine.prepare_transaction_commit(committed_tx)?;
        engine.complete_transaction_commit(committed);
        assert!(
            engine
                .transaction_manager
                .get_transaction_context(committed_tx)
                .is_none()
        );
        assert!(engine.get_node(committed_node).await?.is_some());

        let stats = engine.transaction_manager.stats();
        assert_eq!(stats.active, 0);
        assert_eq!(stats.committed, 1);
        assert_eq!(stats.aborted, 1);
        Ok(())
    }

    #[tokio::test]
    async fn prepared_commit_retains_old_and_new_projection_clone_headroom()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = GraphEngine::new(
            GraphEngineConfig::default().with_memory_limit(MemoryLimiterConfig {
                max_memory_bytes: 1024 * 1024,
                auto_cleanup_enabled: false,
                ..MemoryLimiterConfig::default()
            }),
            Arc::new(InMemoryCanonicalGraphStore::default()),
        )
        .await?;
        let create_tx = engine.begin_transaction()?;
        let node_id = engine
            .create_node_internal(create_tx, vec!["Document".to_string()])
            .await?;
        engine
            .update_node_internal(create_tx, node_id, |node| {
                node.set_property("body", Value::String("x".repeat(16 * 1024)));
            })
            .await?;
        engine.commit_transaction(create_tx)?;

        let update_tx = engine.begin_transaction()?;
        engine
            .update_node_internal(update_tx, node_id, |node| {
                node.set_property("body", Value::String("small".to_string()));
            })
            .await?;
        let overlay_bytes = engine
            .transaction_overlays
            .read()
            .get(&update_tx)
            .ok_or_else(|| std::io::Error::other("transaction overlay missing"))?
            .memory_size();

        let prepared = engine.prepare_transaction_commit(update_tx)?;
        let delta_bytes = prepared.delta.memory_size();
        assert!(overlay_bytes > delta_bytes);
        let prepared_bytes = prepared
            ._prepared_memory
            .as_ref()
            .ok_or_else(|| std::io::Error::other("prepared memory owner missing"))?
            .bytes();
        assert_eq!(
            prepared_bytes,
            delta_bytes.saturating_add(overlay_bytes.max(delta_bytes))
        );

        engine.abort_transaction_commit(prepared);
        assert_eq!(
            category_current(&engine, MemoryCategory::TransactionOverlay),
            0
        );
        Ok(())
    }

    #[tokio::test]
    async fn prepared_commit_failure_aborts_and_releases_transaction_resources()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = GraphEngine::with_temp_dir().await?;
        let tx_id = engine.begin_transaction()?;
        engine
            .create_node_internal(tx_id, vec!["Person".to_string()])
            .await?;

        engine.transaction_overlays.write().remove(&tx_id);
        let error = engine
            .prepare_transaction_commit(tx_id)
            .err()
            .ok_or_else(|| std::io::Error::other("prepare unexpectedly succeeded"))?;

        assert!(error.to_string().contains("overlay"));
        assert!(
            engine
                .transaction_manager
                .get_transaction_context(tx_id)
                .is_none()
        );
        assert!(engine.get_isolation_level(tx_id).is_none());
        assert!(
            !engine
                .mvcc_engine
                .get_active_transactions()
                .contains(&tx_id)
        );
        Ok(())
    }

    #[tokio::test]
    async fn deadlock_victim_is_aborted_before_error_returns()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = Arc::new(GraphEngine::with_temp_dir().await?);
        let first = engine.begin_transaction()?;
        let victim = engine.begin_transaction()?;
        let left = LockResource::Node(101);
        let right = LockResource::Node(202);

        engine
            .acquire_transaction_lock(first, left, LockMode::Exclusive)
            .await?;
        engine
            .acquire_transaction_lock(victim, right, LockMode::Exclusive)
            .await?;

        let waiting_engine = Arc::clone(&engine);
        let waiting = tokio::spawn(async move {
            waiting_engine
                .acquire_transaction_lock(first, right, LockMode::Exclusive)
                .await
        });

        for _ in 0..100 {
            if engine
                .lock_manager
                .get_wait_graph()
                .iter()
                .any(|(waiter, resource, blockers)| {
                    *waiter == first && *resource == right && blockers.contains(&victim)
                })
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(engine.lock_manager.waiter_count(), 1);

        let error = engine
            .acquire_transaction_lock(victim, left, LockMode::Exclusive)
            .await
            .err()
            .ok_or_else(|| std::io::Error::other("cycle-closing request unexpectedly succeeded"))?;
        assert!(matches!(
            error,
            GraphError::Transaction(TransactionError::DeadlockDetected(_))
        ));

        assert!(
            engine
                .transaction_manager
                .get_transaction_context(victim)
                .is_none()
        );
        assert_eq!(engine.lock_manager.locks_held_by(victim), 0);
        assert!(
            engine
                .lock_manager
                .get_wait_graph()
                .iter()
                .all(|(waiter, _, _)| *waiter != victim)
        );
        assert_eq!(engine.lock_manager.wait_graph_size(), 0);
        assert_eq!(engine.transaction_manager.stats().aborted, 1);
        assert!(!engine.transaction_overlays.read().contains_key(&victim));
        assert!(!engine.tx_isolation_levels.read().contains_key(&victim));
        assert!(
            !engine
                .mvcc_engine
                .get_active_transactions()
                .contains(&victim)
        );

        waiting.await??;
        assert_eq!(engine.lock_manager.waiter_count(), 0);
        engine.abort_transaction(victim)?;
        assert_eq!(engine.transaction_manager.stats().aborted, 1);
        engine.abort_transaction(first)?;
        Ok(())
    }
}
