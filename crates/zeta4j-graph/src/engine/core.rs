//! GraphEngine 핵심 구조체 및 생성자

use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tracing::info;

use crate::core::{Edge, Node};
use crate::error::Result;
use crate::index::composite::CompositeIndexManager;
use crate::index::{FunctionIndexManager, SecondaryIndexManager, SpatialIndexManager};
use crate::schema::{
    ConstraintDefinition, ConstraintManager, NodeLabelSchema, RelationshipTypeSchema,
    SchemaManager, SchemaMode,
};
use crate::storage::memory::{
    MemoryCategory, MemoryLimiter, MemoryPool, MemoryReservation, MemoryStats, MemoryStatus,
};
use crate::transaction::{
    IsolationLevel, LockManager, MvccConfig, MvccEngine, TransactionId, TransactionManager,
    VersionInfo,
};

use super::config::{AtomicEngineStats, EngineStats, GraphEngineConfig};
use super::transaction::GraphTransactionOverlay;
use super::{CanonicalGraphStore, InMemoryCanonicalGraphStore};

/// 엔티티 타입 (버전 스토어용)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityType {
    Node,
    Edge,
}

/// 버전 스토어 키
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VersionKey {
    pub entity_type: EntityType,
    pub entity_id: u64,
}

impl VersionKey {
    pub fn node(id: u64) -> Self {
        Self {
            entity_type: EntityType::Node,
            entity_id: id,
        }
    }

    pub fn edge(id: u64) -> Self {
        Self {
            entity_type: EntityType::Edge,
            entity_id: id,
        }
    }
}

/// 통합 그래프 엔진 (설계 문서 준수)
pub struct GraphEngine {
    /// Canonical committed graph. Page storage is not a source of truth.
    pub(crate) canonical_store: Arc<dyn CanonicalGraphStore>,

    /// Derived committed runtime projection, rebuilt from canonical_store.
    pub(crate) committed_nodes: Arc<RwLock<HashMap<u64, Node>>>,
    pub(crate) committed_edges: Arc<RwLock<HashMap<u64, Edge>>>,

    /// Canonical identity allocators. Rebuilt from committed entity maxima.
    pub(crate) next_node_id: AtomicU64,
    pub(crate) next_edge_id: AtomicU64,

    /// Serializes canonical index DDL with runtime projection installation.
    pub(crate) index_definition_lock: Arc<Mutex<()>>,

    /// 메모리 풀 (Node용)
    pub(crate) node_pool: Arc<MemoryPool<Node>>,

    /// 메모리 풀 (Edge용)
    pub(crate) edge_pool: Arc<MemoryPool<Edge>>,

    /// 엔진 통계 (Lock-free)
    pub(crate) stats: Arc<AtomicEngineStats>,

    /// 엔진 설정
    pub(crate) config: GraphEngineConfig,

    /// 생성 시점
    pub(crate) created_at: Instant,

    // 트랜잭션 컴포넌트
    pub(crate) transaction_manager: Arc<TransactionManager>,
    pub(crate) lock_manager: Arc<LockManager>,
    pub(crate) mvcc_engine: Arc<MvccEngine>,

    /// Secondary Index Manager - 프로퍼티 기반 보조 인덱스
    pub(crate) secondary_index_manager: SecondaryIndexManager,

    /// Composite Index Manager - 복합 인덱스 관리
    pub(crate) composite_index_manager: Arc<RwLock<CompositeIndexManager>>,

    /// 스키마 관리자 - 노드/관계 타입 스키마 정의 및 검증
    pub(crate) schema_manager: Arc<RwLock<SchemaManager>>,

    /// 공간 인덱스 관리자 - R-Tree 기반 공간 인덱싱
    pub(crate) spatial_index_manager: SpatialIndexManager,

    /// 제약조건 관리자 - UNIQUE, NOT NULL, NODE KEY 등
    pub constraint_manager: Arc<RwLock<ConstraintManager>>,

    /// 메모리 제한 관리자
    pub(crate) memory_limiter: Option<Arc<MemoryLimiter>>,

    /// Long-lived ownership of the committed in-memory projection.
    pub(crate) committed_memory: Mutex<Option<MemoryReservation>>,

    /// Long-lived ownership of production in-memory secondary indexes.
    pub(crate) index_memory: Mutex<Option<MemoryReservation>>,

    /// Transaction overlay ownership, released by every terminal transaction path.
    pub(crate) transaction_memory: Mutex<HashMap<TransactionId, MemoryReservation>>,

    /// 함수 인덱스 관리자 - 계산된 속성 인덱싱
    pub(crate) function_index_manager: FunctionIndexManager,

    /// MVCC 버전 스토어 - 엔티티별 버전 정보 관리
    pub(crate) version_store: Arc<RwLock<HashMap<VersionKey, VersionInfo>>>,

    /// 트랜잭션별 격리 수준 저장
    pub(crate) tx_isolation_levels: Arc<RwLock<HashMap<TransactionId, IsolationLevel>>>,

    /// Transaction-local canonical entity changes used to build commit deltas.
    pub(crate) transaction_overlays: Arc<RwLock<HashMap<TransactionId, GraphTransactionOverlay>>>,
}

impl GraphEngine {
    /// Rebuild every derived committed projection after the canonical store was
    /// atomically replaced (for example by a backup restore).
    pub async fn reload_canonical_projection(&self) -> Result<()> {
        let mut nodes = HashMap::new();
        for node_id in self.canonical_store.all_node_ids()? {
            if let Some(node) = self.canonical_store.node(node_id)? {
                nodes.insert(node_id, node);
            }
        }
        let mut edges = HashMap::new();
        for edge_id in self.canonical_store.all_edge_ids()? {
            if let Some(edge) = self.canonical_store.edge(edge_id)? {
                edges.insert(edge_id, edge);
            }
        }
        let next_node_id = nodes
            .keys()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| crate::error::GraphError::System {
                message: "canonical node ID space is exhausted".to_string(),
            })?;
        let next_edge_id = edges
            .keys()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| crate::error::GraphError::System {
                message: "canonical edge ID space is exhausted".to_string(),
            })?;
        let definitions = self.canonical_store.index_definitions()?;
        let measured = Self::projection_memory_usage(&nodes, &edges);
        let committed_before = self
            .committed_memory
            .lock()
            .as_ref()
            .map_or(0, MemoryReservation::bytes);
        let index_before = self
            .index_memory
            .lock()
            .as_ref()
            .map_or(0, MemoryReservation::bytes);
        let index_headroom = Self::index_build_headroom(&definitions, &nodes);
        if measured > committed_before
            && let Some(reservation) = self.committed_memory.lock().as_mut()
        {
            reservation.resize(measured)?;
        }
        if index_headroom > index_before
            && let Some(reservation) = self.index_memory.lock().as_mut()
            && let Err(error) = reservation.resize(index_headroom)
        {
            if let Some(committed) = self.committed_memory.lock().as_mut() {
                let _ = committed.resize(committed_before);
            }
            return Err(error.into());
        }

        let mut committed_nodes = self.committed_nodes.write();
        let mut committed_edges = self.committed_edges.write();
        *committed_nodes = nodes;
        *committed_edges = edges;
        self.next_node_id.store(next_node_id, Ordering::SeqCst);
        self.next_edge_id.store(next_edge_id, Ordering::SeqCst);
        drop(committed_edges);
        drop(committed_nodes);

        self.secondary_index_manager.clear();
        self.composite_index_manager.write().clear_all();
        self.function_index_manager.clear();
        self.spatial_index_manager.clear();
        self.restore_canonical_index_definitions(definitions)?;
        self.sync_index_memory_reservation()?;
        if measured < committed_before
            && let Some(reservation) = self.committed_memory.lock().as_mut()
        {
            reservation.resize(measured)?;
        }
        Ok(())
    }

    /// 새 그래프 엔진 생성
    ///
    /// # Errors
    /// - canonical store 조회 또는 derived index 구성 실패 시
    pub async fn new(
        config: GraphEngineConfig,
        canonical_store: Arc<dyn CanonicalGraphStore>,
    ) -> Result<Self> {
        let mut committed_nodes = HashMap::new();
        for node_id in canonical_store.all_node_ids()? {
            if let Some(node) = canonical_store.node(node_id)? {
                committed_nodes.insert(node_id, node);
            }
        }
        let mut committed_edges = HashMap::new();
        for edge_id in canonical_store.all_edge_ids()? {
            if let Some(edge) = canonical_store.edge(edge_id)? {
                committed_edges.insert(edge_id, edge);
            }
        }
        let index_definitions = canonical_store.index_definitions()?;
        let next_node_id = committed_nodes
            .keys()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| crate::error::GraphError::System {
                message: "canonical node ID space is exhausted".to_string(),
            })?;
        let next_edge_id = committed_edges
            .keys()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| crate::error::GraphError::System {
                message: "canonical edge ID space is exhausted".to_string(),
            })?;

        // 트랜잭션 컴포넌트 초기화
        let transaction_manager = Arc::new(TransactionManager::new());
        let lock_manager = Arc::new(LockManager::new());
        let mvcc_config = MvccConfig {
            max_write_sets: config.mvcc_max_write_sets,
            snapshot_cache_size: config.mvcc_snapshot_cache_size,
        };
        let mvcc_engine = Arc::new(MvccEngine::with_config(mvcc_config));

        // 메모리 제한 관리자 생성
        let memory_limiter = config
            .memory_limit
            .as_ref()
            .map(|limit_config| Arc::new(MemoryLimiter::new(limit_config.clone())));
        // Only idle pooled objects belong to ObjectPool. Checked-out objects
        // transfer into transaction/committed owners through the CRUD path.
        let node_pool = memory_limiter.as_ref().map_or_else(
            || MemoryPool::new(|| Node::new(0), 1000),
            |limiter| MemoryPool::new_accounted(|| Node::new(0), 1000, Arc::clone(limiter)),
        );
        let edge_pool = memory_limiter.as_ref().map_or_else(
            || MemoryPool::new(|| Edge::new(0, "temp", 0, 0), 1000),
            |limiter| {
                MemoryPool::new_accounted(|| Edge::new(0, "temp", 0, 0), 1000, Arc::clone(limiter))
            },
        );
        let committed_bytes = Self::projection_memory_usage(&committed_nodes, &committed_edges);
        let index_headroom = Self::index_build_headroom(&index_definitions, &committed_nodes);
        let committed_memory = memory_limiter
            .as_ref()
            .map(|limiter| limiter.try_reserve(MemoryCategory::CommittedGraph, committed_bytes))
            .transpose()?;
        let index_memory = memory_limiter
            .as_ref()
            .map(|limiter| limiter.try_reserve(MemoryCategory::Index, index_headroom))
            .transpose()?;

        let engine = Self {
            canonical_store,
            committed_nodes: Arc::new(RwLock::new(committed_nodes)),
            committed_edges: Arc::new(RwLock::new(committed_edges)),
            next_node_id: AtomicU64::new(next_node_id),
            next_edge_id: AtomicU64::new(next_edge_id),
            index_definition_lock: Arc::new(Mutex::new(())),
            node_pool,
            edge_pool,
            stats: Arc::new(AtomicEngineStats::new()),
            config,
            created_at: Instant::now(),
            transaction_manager,
            lock_manager,
            mvcc_engine,
            secondary_index_manager: SecondaryIndexManager::new(),
            composite_index_manager: Arc::new(RwLock::new(CompositeIndexManager::new())),
            schema_manager: Arc::new(RwLock::new(SchemaManager::new())),
            spatial_index_manager: SpatialIndexManager::new(),
            constraint_manager: Arc::new(RwLock::new(ConstraintManager::new())),
            memory_limiter,
            committed_memory: Mutex::new(committed_memory),
            index_memory: Mutex::new(index_memory),
            transaction_memory: Mutex::new(HashMap::new()),
            function_index_manager: FunctionIndexManager::new(),
            version_store: Arc::new(RwLock::new(HashMap::new())),
            tx_isolation_levels: Arc::new(RwLock::new(HashMap::new())),
            transaction_overlays: Arc::new(RwLock::new(HashMap::new())),
        };

        // 메모리 제한 관리자에 cleanup 콜백 설정
        // Note: compact()는 async이므로 sync 콜백에서 호출 불가 - 풀 정리만 수행
        if let Some(ref limiter) = engine.memory_limiter {
            let node_pool = engine.node_pool.clone();
            let edge_pool = engine.edge_pool.clone();

            limiter.set_cleanup_callback(move || {
                node_pool.cleanup();
                edge_pool.cleanup();
            });
        }

        engine.restore_canonical_index_definitions(index_definitions)?;
        engine.sync_index_memory_reservation()?;

        Ok(engine)
    }

    /// 기본 설정으로 그래프 엔진 생성
    ///
    /// # Errors
    /// - WAL 파일 열기 실패 시
    pub async fn with_default() -> Result<Self> {
        Self::new(
            GraphEngineConfig::default(),
            Arc::new(InMemoryCanonicalGraphStore::default()),
        )
        .await
    }

    /// 임시 디렉토리를 사용하는 그래프 엔진 생성 (테스트용)
    ///
    /// v1.9.1: Memory 백엔드 제거로 tempdir 기반 Disk 모드 사용
    /// 테스트에서 디스크 경로 충돌을 피하기 위해 임시 디렉토리 사용
    pub async fn with_temp_dir() -> Result<Self> {
        let temp_dir = tempfile::tempdir().map_err(|e| {
            std::io::Error::other(format!("Failed to create temp directory: {}", e))
        })?;
        let config = GraphEngineConfig {
            data_path: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        // tempdir을 leak해서 테스트 동안 유지
        std::mem::forget(temp_dir);
        Self::new(config, Arc::new(InMemoryCanonicalGraphStore::default())).await
    }

    /// 엔진 통계 조회 (Lock-free 스냅샷)
    pub async fn stats(&self) -> EngineStats {
        let mut stats = self.stats.snapshot();

        stats.memory_usage = self.memory_limiter.as_ref().map_or_else(
            || self.measured_memory_usage(),
            |limiter| limiter.current_usage(),
        );

        stats
    }

    /// 엔진 설정 조회
    pub fn config(&self) -> &GraphEngineConfig {
        &self.config
    }

    /// 노드 개수
    pub async fn node_count(&self) -> Result<usize> {
        Ok(self.committed_nodes.read().len())
    }

    /// Node labels preserve their canonical insertion order.
    pub fn sort_labels_by_order(&self, labels: &[String]) -> Vec<String> {
        labels.to_vec()
    }

    /// 엣지 개수
    pub async fn edge_count(&self) -> usize {
        self.committed_edges.read().len()
    }

    /// 모든 노드 ID 반환
    pub async fn all_node_ids(&self) -> Vec<u64> {
        let mut ids: Vec<_> = self.committed_nodes.read().keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// 모든 엣지 ID 반환
    pub async fn all_edge_ids(&self) -> Vec<u64> {
        let mut ids: Vec<_> = self.committed_edges.read().keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub(crate) fn allocate_node_id(&self) -> Result<u64> {
        self.next_node_id
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |next| {
                next.checked_add(1)
            })
            .map_err(|_| crate::error::GraphError::System {
                message: "canonical node ID space is exhausted".to_string(),
            })
    }

    pub(crate) fn allocate_edge_id(&self) -> Result<u64> {
        self.next_edge_id
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |next| {
                next.checked_add(1)
            })
            .map_err(|_| crate::error::GraphError::System {
                message: "canonical edge ID space is exhausted".to_string(),
            })
    }

    pub(crate) fn observe_node_id(&self, node_id: u64) -> Result<()> {
        let next = node_id
            .checked_add(1)
            .ok_or_else(|| crate::error::GraphError::System {
                message: "canonical node ID space is exhausted".to_string(),
            })?;
        self.next_node_id.fetch_max(next, Ordering::SeqCst);
        Ok(())
    }

    pub(crate) fn observe_edge_id(&self, edge_id: u64) -> Result<()> {
        let next = edge_id
            .checked_add(1)
            .ok_or_else(|| crate::error::GraphError::System {
                message: "canonical edge ID space is exhausted".to_string(),
            })?;
        self.next_edge_id.fetch_max(next, Ordering::SeqCst);
        Ok(())
    }

    /// 노드/엣지 ID 카운터를 현재 최대 ID+1로 동기화
    ///
    /// 지정 ID로 노드/엣지를 생성한 후 반드시 호출해야 한다.
    /// 호출하지 않으면 이후 auto-generate된 ID가 기존 ID와 충돌할 수 있다.
    pub async fn sync_id_counters(&self) -> Result<()> {
        let next_node_id = self
            .committed_nodes
            .read()
            .keys()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| crate::error::GraphError::System {
                message: "canonical node ID space is exhausted".to_string(),
            })?;
        let next_edge_id = self
            .committed_edges
            .read()
            .keys()
            .copied()
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| crate::error::GraphError::System {
                message: "canonical edge ID space is exhausted".to_string(),
            })?;
        self.next_node_id.fetch_max(next_node_id, Ordering::SeqCst);
        self.next_edge_id.fetch_max(next_edge_id, Ordering::SeqCst);
        Ok(())
    }

    /// 엔진 가동 시간
    pub fn uptime(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// 메모리 정리 - 정기적 유지보수
    pub async fn cleanup_memory(&self) {
        self.node_pool.cleanup();
        self.edge_pool.cleanup();
    }

    // ========== 트랜잭션 컴포넌트 접근 ==========

    /// MVCC 엔진 조회 - Snapshot Isolation 및 버전 관리
    pub fn mvcc_engine(&self) -> &Arc<MvccEngine> {
        &self.mvcc_engine
    }

    /// 트랜잭션 매니저 조회
    pub fn transaction_manager(&self) -> &Arc<TransactionManager> {
        &self.transaction_manager
    }

    /// 락 매니저 조회
    pub fn lock_manager(&self) -> &Arc<LockManager> {
        &self.lock_manager
    }

    // ========== 메모리 제한 관리 ==========

    /// 메모리 제한 관리자 조회
    pub fn memory_limiter(&self) -> Option<&Arc<MemoryLimiter>> {
        self.memory_limiter.as_ref()
    }

    /// 현재 메모리 상태 조회
    pub fn memory_status(&self) -> MemoryStatus {
        self.memory_limiter
            .as_ref()
            .map(|l| l.status())
            .unwrap_or(MemoryStatus::Normal)
    }

    /// 메모리 제한 통계 조회
    pub fn memory_limiter_stats(&self) -> Option<MemoryStats> {
        self.memory_limiter.as_ref().map(|l| l.stats())
    }

    /// Compare deterministic container measurement with the ledger without
    /// overwriting reservations that may be changing concurrently.
    pub fn reconcile_memory_usage(&self) -> Option<isize> {
        self.memory_limiter.as_ref().map(|limiter| {
            let stats = limiter.stats();
            let transient = stats
                .category(MemoryCategory::TransactionOverlay)
                .map_or(0, |category| category.current_usage)
                .saturating_add(
                    stats
                        .category(MemoryCategory::Query)
                        .map_or(0, |category| category.current_usage),
                );
            // Query and transaction owners are transient and not discoverable
            // by scanning long-lived engine containers. Add their authoritative
            // ledger values to the measured side so reconciliation reports drift
            // only for scannable long-lived containers.
            limiter.record_reconciliation(self.measured_memory_usage().saturating_add(transient))
        })
    }

    /// Create a scoped reservation for engine consumers such as query execution.
    /// Disabled engines return `None` without touching atomics.
    pub fn try_reserve_memory(
        &self,
        category: MemoryCategory,
        bytes: usize,
    ) -> Result<Option<MemoryReservation>> {
        Ok(self
            .memory_limiter
            .as_ref()
            .map(|limiter| limiter.try_reserve(category, bytes))
            .transpose()?)
    }

    /// 메모리 사용률 조회 (0.0 ~ 1.0)
    pub fn memory_usage_ratio(&self) -> f64 {
        self.memory_limiter
            .as_ref()
            .map(|l| l.usage_ratio())
            .unwrap_or(0.0)
    }

    pub(crate) fn measured_memory_usage(&self) -> usize {
        let nodes = self.committed_nodes.read();
        let edges = self.committed_edges.read();
        Self::projection_memory_usage(&nodes, &edges)
            .saturating_add(self.node_pool.estimated_memory_usage())
            .saturating_add(self.edge_pool.estimated_memory_usage())
            .saturating_add(self.measured_index_memory_usage())
    }

    fn projection_memory_usage(nodes: &HashMap<u64, Node>, edges: &HashMap<u64, Edge>) -> usize {
        nodes
            .values()
            .map(Node::memory_size)
            .chain(edges.values().map(Edge::memory_size))
            .fold(0usize, usize::saturating_add)
    }

    pub(crate) fn measured_index_memory_usage(&self) -> usize {
        self.secondary_index_manager
            .memory_size()
            .saturating_add(self.composite_index_manager.read().memory_size())
            .saturating_add(self.function_index_manager.memory_size())
            .saturating_add(self.spatial_index_manager.memory_size())
    }

    pub(crate) fn index_build_headroom(
        definitions: &[super::GraphIndexDefinition],
        nodes: &HashMap<u64, Node>,
    ) -> usize {
        definitions
            .iter()
            .filter(|definition| {
                !matches!(
                    &definition.kind,
                    super::GraphIndexKind::Fulltext { .. } | super::GraphIndexKind::Vector { .. }
                )
            })
            .map(|definition| {
                let definition_bytes = std::mem::size_of::<super::GraphIndexDefinition>()
                    .saturating_add(definition.name.len())
                    .saturating_add(definition.label.len())
                    .saturating_add(
                        definition
                            .properties
                            .iter()
                            .map(|property| {
                                std::mem::size_of::<String>().saturating_add(property.len())
                            })
                            .fold(0usize, usize::saturating_add),
                    );
                let matching_nodes = nodes
                    .values()
                    .filter(|node| node.labels().contains(&definition.label))
                    .map(Node::memory_size)
                    .fold(0usize, usize::saturating_add);
                definition_bytes
                    .saturating_add(matching_nodes.saturating_mul(4))
                    .saturating_add(4096)
            })
            .fold(0usize, usize::saturating_add)
    }

    pub(crate) fn sync_index_memory_reservation(&self) -> Result<()> {
        let measured = self.measured_index_memory_usage();
        if let Some(reservation) = self.index_memory.lock().as_mut() {
            reservation.resize(measured)?;
        }
        Ok(())
    }

    /// Canonical stores own durability; GraphEngine has no independent flush boundary.
    pub async fn shutdown(&self) -> Result<()> {
        info!("GraphEngine shutting down...");
        info!("GraphEngine shutdown complete");
        Ok(())
    }

    /// 노드에 연결된 엣지가 있는지 확인
    pub async fn has_connected_edges(&self, node_id: u64) -> bool {
        self.committed_edges
            .read()
            .values()
            .any(|edge| edge.connects_to(node_id))
    }

    /// 노드에 연결된 엣지 ID 목록 조회
    pub async fn get_connected_edges(&self, node_id: u64) -> Vec<u64> {
        let mut ids: Vec<_> = self
            .committed_edges
            .read()
            .values()
            .filter(|edge| edge.connects_to(node_id))
            .map(Edge::id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// 노드에서 나가는 엣지 ID 목록 조회
    pub async fn get_outgoing_edges(&self, node_id: u64) -> Vec<u64> {
        let mut ids: Vec<_> = self
            .committed_edges
            .read()
            .values()
            .filter(|edge| edge.from_node() == node_id)
            .map(Edge::id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// 노드로 들어오는 엣지 ID 목록 조회
    pub async fn get_incoming_edges(&self, node_id: u64) -> Vec<u64> {
        let mut ids: Vec<_> = self
            .committed_edges
            .read()
            .values()
            .filter(|edge| edge.to_node() == node_id)
            .map(Edge::id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// 두 노드 사이의 엣지 ID 목록 조회
    pub async fn get_edges_between(&self, from: u64, to: u64) -> Vec<u64> {
        let mut ids: Vec<_> = self
            .committed_edges
            .read()
            .values()
            .filter(|edge| edge.from_node() == from && edge.to_node() == to)
            .map(Edge::id)
            .collect();
        ids.sort_unstable();
        ids
    }

    // ========== 스키마 관리 ==========

    /// 스키마 모드 설정
    ///
    /// - `Flexible`: 스키마 검증 안 함 (기본값)
    /// - `Validate`: 정의된 스키마만 타입 검증
    /// - `Strict`: 스키마에 정의되지 않은 프로퍼티 거부
    pub fn set_schema_mode(&self, mode: SchemaMode) {
        self.schema_manager.write().set_mode(mode);
    }

    /// 현재 스키마 모드 조회
    pub fn schema_mode(&self) -> SchemaMode {
        self.schema_manager.read().mode()
    }

    /// 노드 라벨 스키마 정의
    pub fn define_node_label_schema(&self, schema: NodeLabelSchema) -> Result<()> {
        self.schema_manager.write().define_node_label(schema)
    }

    /// 노드 라벨 스키마 조회
    pub fn get_node_label_schema(&self, label: &str) -> Option<NodeLabelSchema> {
        self.schema_manager
            .read()
            .get_node_label_schema(label)
            .cloned()
    }

    /// 노드 라벨 스키마 삭제
    pub fn drop_node_label_schema(&self, label: &str) -> Result<()> {
        self.schema_manager.write().drop_node_label_schema(label)
    }

    /// 모든 노드 라벨 목록 조회
    pub fn list_node_label_schemas(&self) -> Vec<String> {
        self.schema_manager
            .read()
            .list_node_labels()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// 관계 타입 스키마 정의
    pub fn define_relationship_type_schema(&self, schema: RelationshipTypeSchema) -> Result<()> {
        self.schema_manager.write().define_relationship_type(schema)
    }

    /// 관계 타입 스키마 조회
    pub fn get_relationship_type_schema(&self, rel_type: &str) -> Option<RelationshipTypeSchema> {
        self.schema_manager
            .read()
            .get_relationship_type_schema(rel_type)
            .cloned()
    }

    /// 관계 타입 스키마 삭제
    pub fn drop_relationship_type_schema(&self, rel_type: &str) -> Result<()> {
        self.schema_manager
            .write()
            .drop_relationship_type_schema(rel_type)
    }

    /// 모든 관계 타입 목록 조회
    pub fn list_relationship_type_schemas(&self) -> Vec<String> {
        self.schema_manager
            .read()
            .list_relationship_types()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// 스키마를 JSON으로 직렬화
    pub fn schema_to_json(&self) -> Result<String> {
        self.schema_manager.read().to_json()
    }

    /// JSON에서 스키마 로드
    pub fn load_schema_from_json(&self, json: &str) -> Result<()> {
        let schema = SchemaManager::from_json(json)?;
        *self.schema_manager.write() = schema;
        Ok(())
    }

    /// 스키마 초기화 (모든 스키마 삭제)
    pub fn clear_schema(&self) {
        self.schema_manager.write().clear();
    }

    // ========== 제약조건 관리 메서드 ==========

    /// 제약조건 추가
    pub fn add_constraint(&self, constraint: ConstraintDefinition) -> Result<()> {
        self.constraint_manager.write().add_constraint(constraint)
    }

    /// 제약조건 삭제
    pub fn remove_constraint(&self, name: &str) -> Result<()> {
        self.constraint_manager.write().remove_constraint(name)?;
        Ok(())
    }

    /// 제약조건 조회
    pub fn get_constraint(&self, name: &str) -> Result<Option<ConstraintDefinition>> {
        self.constraint_manager.read().get_constraint(name)
    }

    /// 모든 제약조건 목록 조회
    pub fn list_constraints(&self) -> Vec<ConstraintDefinition> {
        self.constraint_manager
            .read()
            .list_constraints()
            .unwrap_or_default()
    }

    /// 특정 라벨에 대한 제약조건 조회
    pub fn get_constraints_for_label(&self, label: &str) -> Vec<ConstraintDefinition> {
        self.constraint_manager
            .read()
            .get_constraints_for_label(label)
            .unwrap_or_default()
    }
}

impl std::fmt::Debug for GraphEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphEngine")
            .field("config", &self.config)
            .field("uptime", &self.uptime())
            .finish()
    }
}

// ========== GraphView 트레이트 구현 ==========

impl super::graph_view::GraphView for GraphEngine {
    async fn node_count(&self) -> Result<usize> {
        Ok(self.committed_nodes.read().len())
    }

    async fn edge_count(&self) -> usize {
        self.committed_edges.read().len()
    }

    async fn get_node(&self, node_id: u64) -> Result<Option<crate::core::Node>> {
        Ok(self.committed_nodes.read().get(&node_id).cloned())
    }

    async fn get_edge(&self, edge_id: u64) -> Result<Option<crate::core::Edge>> {
        Ok(self.committed_edges.read().get(&edge_id).cloned())
    }

    async fn find_edges_by_node(&self, node_id: u64) -> Result<Vec<u64>> {
        Ok(self.get_connected_edges(node_id).await)
    }

    async fn find_outgoing_edges(&self, node_id: u64) -> Vec<u64> {
        self.get_outgoing_edges(node_id).await
    }

    async fn find_incoming_edges(&self, node_id: u64) -> Vec<u64> {
        self.get_incoming_edges(node_id).await
    }

    async fn all_node_ids(&self) -> Vec<u64> {
        self.all_node_ids().await
    }

    async fn all_edge_ids(&self) -> Vec<u64> {
        self.all_edge_ids().await
    }

    async fn find_edges_between(&self, from: u64, to: u64) -> Vec<u64> {
        self.get_edges_between(from, to).await
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;
    use crate::engine::{
        GraphIndexDefinition, GraphIndexKind, GraphTransactionDelta, InMemoryCanonicalGraphStore,
    };
    use crate::storage::memory::MemoryLimiterConfig;

    fn committed_bytes(engine: &GraphEngine) -> usize {
        engine
            .memory_limiter_stats()
            .and_then(|stats| {
                stats
                    .category(MemoryCategory::CommittedGraph)
                    .map(|category| category.current_usage)
            })
            .unwrap_or(0)
    }

    #[test]
    fn external_index_definitions_do_not_consume_engine_index_headroom() {
        let definitions = vec![
            GraphIndexDefinition {
                name: "search".to_string(),
                label: "Document".to_string(),
                properties: vec!["body".to_string()],
                kind: GraphIndexKind::Fulltext {
                    analyzer: "standard".to_string(),
                },
            },
            GraphIndexDefinition {
                name: "embedding".to_string(),
                label: "Document".to_string(),
                properties: vec!["vector".to_string()],
                kind: GraphIndexKind::Vector {
                    dimensions: 3,
                    distance: "cosine".to_string(),
                },
            },
        ];

        assert_eq!(
            GraphEngine::index_build_headroom(&definitions, &HashMap::new()),
            0
        );
    }

    #[tokio::test]
    async fn startup_reserves_index_build_headroom_before_projection()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let store = Arc::new(InMemoryCanonicalGraphStore::default());
        store.create_index_definition(GraphIndexDefinition {
            name: "by_name".to_string(),
            label: "Person".to_string(),
            properties: vec!["name".to_string()],
            kind: GraphIndexKind::Property,
        })?;
        let rejected = GraphEngine::new(
            GraphEngineConfig::default().with_memory_limit(MemoryLimiterConfig {
                max_memory_bytes: 1024,
                auto_cleanup_enabled: false,
                ..MemoryLimiterConfig::default()
            }),
            store.clone(),
        )
        .await;
        assert!(matches!(rejected, Err(crate::error::GraphError::Memory(_))));

        let accepted = GraphEngine::new(
            GraphEngineConfig::default().with_memory_limit(MemoryLimiterConfig {
                max_memory_bytes: 8192,
                auto_cleanup_enabled: false,
                ..MemoryLimiterConfig::default()
            }),
            store,
        )
        .await?;
        assert!(
            accepted
                .memory_limiter_stats()
                .and_then(|stats| {
                    stats
                        .category(MemoryCategory::Index)
                        .map(|category| category.current_usage)
                })
                .unwrap_or(0)
                > 0
        );
        Ok(())
    }

    #[tokio::test]
    async fn load_restart_and_projection_reload_install_the_same_baseline()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let store = Arc::new(InMemoryCanonicalGraphStore::default());
        let config = GraphEngineConfig::default().with_memory_limit(MemoryLimiterConfig {
            max_memory_bytes: 1024 * 1024,
            auto_cleanup_enabled: false,
            ..MemoryLimiterConfig::default()
        });
        let engine = GraphEngine::new(config.clone(), store.clone()).await?;
        let tx_id = engine.begin_transaction()?;
        engine
            .create_node_internal(tx_id, vec!["Initial".to_string()])
            .await?;
        engine.commit_transaction(tx_id)?;
        let initial = committed_bytes(&engine);
        assert!(initial > 0);
        assert_eq!(engine.reconcile_memory_usage(), Some(0));
        drop(engine);

        let restarted = GraphEngine::new(config, store.clone()).await?;
        assert_eq!(committed_bytes(&restarted), initial);
        let mut restored_node = Node::new(77);
        restored_node.add_label("Restored");
        store.commit_graph(GraphTransactionDelta {
            upsert_nodes: vec![restored_node],
            ..GraphTransactionDelta::default()
        })?;
        restarted.reload_canonical_projection().await?;
        assert!(committed_bytes(&restarted) > initial);
        assert_eq!(restarted.reconcile_memory_usage(), Some(0));
        Ok(())
    }
}
