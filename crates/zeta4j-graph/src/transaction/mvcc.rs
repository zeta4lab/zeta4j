// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # MVCC (Multi-Version Concurrency Control)
//!
//! 다중 버전 동시성 제어를 제공하는 모듈입니다.
//!
//! ## 개요
//!
//! MVCC는 데이터의 여러 버전을 유지하여 읽기와 쓰기가 서로 차단하지 않도록 합니다.
//! 각 트랜잭션은 시작 시점의 "스냅샷"을 보며, 다른 트랜잭션의 변경에 영향받지 않습니다.
//!
//! ## 격리 수준 (Isolation Levels)
//!
//! | 수준 | 설명 | Dirty Read | Non-Repeatable Read | Phantom |
//! |------|------|------------|---------------------|---------|
//! | ReadCommitted | 각 쿼리마다 최신 커밋 데이터 | ❌ | ⭕ | ⭕ |
//! | RepeatableRead | 트랜잭션 시작 시 스냅샷 고정 | ❌ | ❌ | ⭕ |
//! | Serializable | SSI 기반 완전 격리 | ❌ | ❌ | ❌ |
//!
//! ## 주요 구성요소
//!
//! - [`MvccEngine`]: MVCC 엔진 (스냅샷 관리, 가시성 판단)
//! - [`Snapshot`]: 트랜잭션 스냅샷
//! - [`VersionInfo`]: 버전 메타데이터
//! - [`ConflictError`]: Write-Write 충돌 에러
//!
//! ## 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::transaction::{MvccEngine, IsolationLevel};
//!
//! let mvcc = MvccEngine::new();
//!
//! // RepeatableRead로 트랜잭션 시작
//! let snapshot = mvcc.begin_transaction_with_isolation(1, IsolationLevel::RepeatableRead);
//!
//! // 다른 트랜잭션이 커밋해도 스냅샷은 고정
//! mvcc.begin_transaction(2);
//! mvcc.commit_transaction(2);
//!
//! // TX1은 여전히 시작 시점의 데이터를 봄
//! mvcc.commit_transaction(1);
//! ```

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{IsolationLevel, Result, TransactionError, TransactionId};

/// 버전 정보 - 각 데이터 항목의 버전 메타데이터
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    /// 이 버전을 생성한 트랜잭션 ID
    pub created_by: TransactionId,
    /// 이 버전을 삭제한 트랜잭션 ID (None이면 현재 유효)
    pub deleted_by: Option<TransactionId>,
    /// 생성 시점의 커밋 타임스탬프 (0이면 미커밋)
    pub created_ts: u64,
    /// 삭제 시점의 커밋 타임스탬프 (None이면 삭제되지 않음)
    pub deleted_ts: Option<u64>,
}

impl VersionInfo {
    /// 새 버전 정보 생성
    pub fn new(created_by: TransactionId) -> Self {
        Self {
            created_by,
            deleted_by: None,
            created_ts: 0,
            deleted_ts: None,
        }
    }

    /// 삭제 표시
    pub fn mark_deleted(&mut self, deleted_by: TransactionId) {
        self.deleted_by = Some(deleted_by);
    }

    /// 커밋 타임스탬프 설정
    pub fn set_commit_timestamp(&mut self, ts: u64) {
        self.created_ts = ts;
    }

    /// 삭제 커밋 타임스탬프 설정
    pub fn set_delete_timestamp(&mut self, ts: u64) {
        self.deleted_ts = Some(ts);
    }
}

/// 스냅샷 - 트랜잭션 시작 시점의 데이터베이스 상태
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// 이 스냅샷을 가진 트랜잭션 ID
    pub tx_id: TransactionId,
    /// 격리 수준
    pub isolation_level: IsolationLevel,
    /// 스냅샷 생성 시점에 활성화된 트랜잭션들
    pub active_transactions: HashSet<TransactionId>,
    /// 스냅샷 생성 시점의 최소 활성 트랜잭션 ID
    pub min_active_tx: TransactionId,
    /// 스냅샷 생성 시점의 최대 트랜잭션 ID (다음 ID - 1)
    pub max_tx: TransactionId,
    /// 스냅샷 생성 타임스탬프
    pub timestamp: u64,
}

impl Snapshot {
    /// 새 스냅샷 생성
    pub fn new(
        tx_id: TransactionId,
        active_transactions: HashSet<TransactionId>,
        max_tx: TransactionId,
        timestamp: u64,
    ) -> Self {
        Self::with_isolation(
            tx_id,
            active_transactions,
            max_tx,
            timestamp,
            IsolationLevel::ReadCommitted,
        )
    }

    /// 격리 수준을 지정하여 스냅샷 생성
    pub fn with_isolation(
        tx_id: TransactionId,
        active_transactions: HashSet<TransactionId>,
        max_tx: TransactionId,
        timestamp: u64,
        isolation_level: IsolationLevel,
    ) -> Self {
        let min_active_tx = active_transactions.iter().min().copied().unwrap_or(tx_id);
        Self {
            tx_id,
            isolation_level,
            active_transactions,
            min_active_tx,
            max_tx,
            timestamp,
        }
    }
}

/// Write-Write 충돌 에러
#[derive(Debug, Clone)]
pub enum ConflictError {
    /// 두 트랜잭션이 같은 데이터를 수정하려 함
    WriteWriteConflict {
        our_tx: TransactionId,
        their_tx: TransactionId,
    },
    /// 직렬화 실패 (Serializable 격리 수준에서)
    SerializationFailure,
}

impl std::fmt::Display for ConflictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConflictError::WriteWriteConflict { our_tx, their_tx } => {
                write!(
                    f,
                    "Write-Write conflict: TX {} and TX {} both modified the same data",
                    our_tx, their_tx
                )
            }
            ConflictError::SerializationFailure => {
                write!(f, "Serialization failure")
            }
        }
    }
}

impl std::error::Error for ConflictError {}

/// 읽기 세트 항목 (Serializable용)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReadSetItem {
    /// 노드 읽기
    Node(u64),
    /// 엣지 읽기
    Edge(u64),
}

/// MVCC 엔진
///
/// 다중 버전 동시성 제어의 핵심 컴포넌트입니다.
///
/// # 주요 기능
///
/// - **스냅샷 관리**: 트랜잭션별 스냅샷 생성 및 관리
/// - **가시성 판단**: [`check_visibility`](Self::check_visibility)로 버전 가시성 확인
/// - **충돌 감지**: [`check_write_conflict`](Self::check_write_conflict)로 Write-Write 충돌 감지
/// - **SSI 검증**: [`validate_serializable`](Self::validate_serializable)로 직렬화 가능성 검증
/// - **GC 지원**: [`can_gc_version`](Self::can_gc_version), [`gc_commit_timestamps`](Self::gc_commit_timestamps)
///
/// # 격리 수준별 동작
///
/// | 격리 수준 | 스냅샷 갱신 | 읽기/쓰기 추적 | SSI 검증 |
/// |-----------|-------------|----------------|----------|
/// | ReadCommitted | ⭕ (매 쿼리) | ❌ | ❌ |
/// | RepeatableRead | ❌ (고정) | ❌ | ❌ |
/// | Serializable | ❌ (고정) | ⭕ | ⭕ |
///
/// # 스레드 안전성
///
/// 모든 내부 상태는 `RwLock`으로 보호되며, 여러 스레드에서
/// 안전하게 사용할 수 있습니다.
///
/// # 예시
///
/// ```rust,ignore
/// use zeta4j_graph::transaction::{MvccEngine, IsolationLevel};
///
/// let mvcc = MvccEngine::new();
///
/// // 트랜잭션 시작
/// let snapshot = mvcc.begin_transaction_with_isolation(1, IsolationLevel::RepeatableRead);
///
/// // 버전 가시성 확인
/// let version = VersionInfo::new(0);
/// if mvcc.check_visibility(&version, 1)? {
///     // 이 버전이 보임
/// }
///
/// // 커밋
/// let commit_ts = mvcc.commit_transaction(1);
/// ```
/// MVCC 엔진 설정
#[derive(Debug, Clone)]
pub struct MvccConfig {
    /// 커밋된 쓰기 세트 최대 보관 개수 (SSI 검증용)
    pub max_write_sets: usize,
    /// 스냅샷 캐시 최대 크기 (예약됨, 현재 미사용)
    pub snapshot_cache_size: usize,
}

impl Default for MvccConfig {
    fn default() -> Self {
        Self {
            max_write_sets: 100,
            snapshot_cache_size: 1000,
        }
    }
}

pub struct MvccEngine {
    /// 트랜잭션별 스냅샷 (Arc로 공유하여 clone 비용 절감)
    snapshots: RwLock<HashMap<TransactionId, Arc<Snapshot>>>,
    /// 커밋된 트랜잭션의 타임스탬프
    commit_timestamps: RwLock<HashMap<TransactionId, u64>>,
    /// 현재 활성 트랜잭션 목록
    active_transactions: RwLock<HashSet<TransactionId>>,
    /// 다음 타임스탬프
    next_timestamp: RwLock<u64>,
    /// 트랜잭션별 읽기 세트 (Serializable용)
    read_sets: RwLock<HashMap<TransactionId, HashSet<ReadSetItem>>>,
    /// 트랜잭션별 쓰기 세트 (Serializable용)
    write_sets: RwLock<HashMap<TransactionId, HashSet<ReadSetItem>>>,
    /// 커밋된 트랜잭션의 쓰기 세트 (SSI 검증용, 최근 N개만 유지)
    committed_write_sets: RwLock<HashMap<TransactionId, HashSet<ReadSetItem>>>,
    /// MVCC 설정
    config: MvccConfig,
}

impl MvccEngine {
    /// 기본 설정으로 생성
    pub fn new() -> Self {
        Self::with_config(MvccConfig::default())
    }

    /// 설정을 지정하여 생성
    pub fn with_config(config: MvccConfig) -> Self {
        Self {
            snapshots: RwLock::new(HashMap::new()),
            commit_timestamps: RwLock::new(HashMap::new()),
            active_transactions: RwLock::new(HashSet::new()),
            next_timestamp: RwLock::new(1),
            read_sets: RwLock::new(HashMap::new()),
            write_sets: RwLock::new(HashMap::new()),
            committed_write_sets: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// 트랜잭션 시작 시 스냅샷 캡처 (기본: ReadCommitted)
    pub fn begin_transaction(&self, tx_id: TransactionId) -> Snapshot {
        self.begin_transaction_with_isolation(tx_id, IsolationLevel::ReadCommitted)
    }

    /// 격리 수준을 지정하여 트랜잭션 시작
    pub fn begin_transaction_with_isolation(
        &self,
        tx_id: TransactionId,
        isolation_level: IsolationLevel,
    ) -> Snapshot {
        // 활성 트랜잭션 목록을 한 번만 읽기 (clone 최소화)
        let (active, timestamp, max_tx) = {
            let active_read = self.active_transactions.read();
            let ts = *self.next_timestamp.read();
            let max = tx_id.saturating_sub(1);
            (active_read.clone(), ts, max)
        };

        let snapshot = Snapshot::with_isolation(tx_id, active, max_tx, timestamp, isolation_level);
        let snapshot_arc = Arc::new(snapshot.clone());

        // 활성 트랜잭션에 추가
        self.active_transactions.write().insert(tx_id);

        // 스냅샷 저장 (Arc로 공유)
        self.snapshots.write().insert(tx_id, snapshot_arc);

        // 쓰기 세트 항상 초기화 (빈 트랜잭션 감지용)
        self.write_sets.write().insert(tx_id, HashSet::new());

        // Serializable의 경우 읽기 세트도 초기화 (SSI 검증용)
        if isolation_level == IsolationLevel::Serializable {
            self.read_sets.write().insert(tx_id, HashSet::new());
        }

        snapshot
    }

    /// 트랜잭션 커밋
    pub fn commit_transaction(&self, tx_id: TransactionId) -> u64 {
        // 커밋 타임스탬프 할당
        let commit_ts = {
            let mut ts = self.next_timestamp.write();
            let current = *ts;
            *ts += 1;
            current
        };

        // 커밋 타임스탬프 저장
        self.commit_timestamps.write().insert(tx_id, commit_ts);

        // 활성 트랜잭션에서 제거
        self.active_transactions.write().remove(&tx_id);

        // Serializable: 쓰기 세트를 커밋된 쓰기 세트로 이동 (SSI 검증용)
        if let Some(write_set) = self.write_sets.write().remove(&tx_id)
            && !write_set.is_empty()
        {
            self.committed_write_sets.write().insert(tx_id, write_set);
        }
        self.read_sets.write().remove(&tx_id);

        // 스냅샷 제거
        self.snapshots.write().remove(&tx_id);

        // 오래된 커밋된 쓰기 세트 정리 (설정된 개수만 유지)
        self.cleanup_old_committed_write_sets(self.config.max_write_sets);

        commit_ts
    }

    /// 오래된 커밋된 쓰기 세트 정리
    fn cleanup_old_committed_write_sets(&self, max_entries: usize) {
        let mut committed = self.committed_write_sets.write();
        if committed.len() > max_entries {
            // 가장 오래된 항목부터 삭제 (tx_id가 작은 것)
            let mut tx_ids: Vec<_> = committed.keys().copied().collect();
            tx_ids.sort();
            let to_remove = tx_ids.len() - max_entries;
            for tx_id in tx_ids.into_iter().take(to_remove) {
                committed.remove(&tx_id);
            }
        }
    }

    /// 트랜잭션 롤백
    pub fn rollback_transaction(&self, tx_id: TransactionId) {
        self.active_transactions.write().remove(&tx_id);
        self.snapshots.write().remove(&tx_id);
        self.read_sets.write().remove(&tx_id);
        self.write_sets.write().remove(&tx_id);
    }

    /// 스냅샷 조회 (Arc 참조 반환으로 clone 비용 절감)
    pub fn get_snapshot(&self, tx_id: TransactionId) -> Option<Arc<Snapshot>> {
        self.snapshots.read().get(&tx_id).cloned()
    }

    /// 트랜잭션이 커밋되었는지 확인
    pub fn is_committed(&self, tx_id: TransactionId) -> bool {
        self.commit_timestamps.read().contains_key(&tx_id)
    }

    /// 커밋 타임스탬프 조회
    pub fn get_commit_timestamp(&self, tx_id: TransactionId) -> Option<u64> {
        self.commit_timestamps.read().get(&tx_id).copied()
    }

    /// 버전이 주어진 트랜잭션에서 보이는지 확인 (Snapshot Isolation)
    ///
    /// # 가시성 규칙
    /// 1. 자신이 생성한 버전은 항상 보임 (단, 자신이 삭제한 경우 제외)
    /// 2. 생성 트랜잭션이 아직 커밋되지 않았으면 안 보임
    /// 3. 생성 트랜잭션이 스냅샷 이후에 시작되었으면 안 보임
    /// 4. 삭제된 버전이면서 삭제 트랜잭션이 커밋되었고 스냅샷 이전이면 안 보임
    pub fn check_visibility(&self, version: &VersionInfo, tx_id: TransactionId) -> Result<bool> {
        let snapshot_arc = self
            .get_snapshot(tx_id)
            .ok_or(TransactionError::NotFound(tx_id))?;
        let snapshot = snapshot_arc.as_ref();

        // 규칙 1: 자신이 생성한 버전은 항상 보임
        if version.created_by == tx_id {
            // 단, 자신이 삭제한 경우는 안 보임
            return Ok(version.deleted_by != Some(tx_id));
        }

        // 규칙 2: 생성 트랜잭션이 아직 커밋되지 않았으면 안 보임
        if snapshot.active_transactions.contains(&version.created_by) {
            return Ok(false);
        }

        // 규칙 3: 생성 트랜잭션이 스냅샷 이후에 시작되었으면 안 보임
        if version.created_by > snapshot.max_tx {
            return Ok(false);
        }

        // 규칙 4: 삭제된 버전 처리
        if let Some(deleted_by) = version.deleted_by {
            // 자신이 삭제했으면 안 보임
            if deleted_by == tx_id {
                return Ok(false);
            }

            // 삭제 트랜잭션이 커밋되었고, 스냅샷 이전이면 안 보임
            if !snapshot.active_transactions.contains(&deleted_by) && deleted_by <= snapshot.max_tx
            {
                // 추가 확인: 삭제 트랜잭션이 실제로 커밋되었는지
                if self.is_committed(deleted_by) {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    /// 쓰기 충돌 감지 (First-Updater-Wins)
    ///
    /// 다른 트랜잭션이 이미 해당 데이터를 수정했는지 확인합니다.
    /// Write-Write 충돌이 발생하면 현재 트랜잭션은 롤백해야 합니다.
    pub fn check_write_conflict(
        &self,
        version: &VersionInfo,
        tx_id: TransactionId,
    ) -> std::result::Result<(), ConflictError> {
        // 다른 트랜잭션이 이미 삭제(수정)했는지 확인
        if let Some(deleted_by) = version.deleted_by
            && deleted_by != tx_id
        {
            // 충돌 발생
            return Err(ConflictError::WriteWriteConflict {
                our_tx: tx_id,
                their_tx: deleted_by,
            });
        }
        Ok(())
    }

    // ========== ReadCommitted 지원 ==========

    /// ReadCommitted용 스냅샷 갱신
    ///
    /// ReadCommitted에서는 각 읽기마다 최신 커밋된 데이터를 보기 위해
    /// 스냅샷을 갱신합니다.
    pub fn refresh_snapshot(&self, tx_id: TransactionId) -> Option<Snapshot> {
        let mut snapshots = self.snapshots.write();
        if let Some(snapshot_arc) = snapshots.get(&tx_id) {
            // ReadCommitted만 갱신
            if snapshot_arc.isolation_level == IsolationLevel::ReadCommitted {
                let active = self.active_transactions.read().clone();
                let timestamp = *self.next_timestamp.read();
                let max_tx = active.iter().max().copied().unwrap_or(tx_id);

                let new_snapshot = Snapshot::with_isolation(
                    tx_id,
                    active,
                    max_tx,
                    timestamp,
                    IsolationLevel::ReadCommitted,
                );
                let result = new_snapshot.clone();
                snapshots.insert(tx_id, Arc::new(new_snapshot));
                return Some(result);
            }
            // 다른 격리 수준은 스냅샷 복사하여 반환
            return Some((**snapshot_arc).clone());
        }
        None
    }

    // ========== Serializable (SSI) 지원 ==========

    /// 읽기 기록 (Serializable용)
    ///
    /// Serializable 격리 수준에서 읽기 작업을 추적합니다.
    pub fn record_read(&self, tx_id: TransactionId, item: ReadSetItem) {
        if let Some(snapshot_arc) = self.get_snapshot(tx_id)
            && snapshot_arc.isolation_level == IsolationLevel::Serializable
        {
            self.read_sets
                .write()
                .entry(tx_id)
                .or_default()
                .insert(item);
        }
    }

    /// 쓰기 기록
    ///
    /// 모든 격리 수준에서 쓰기 작업을 추적합니다.
    /// - 빈 트랜잭션 감지: `has_write_set()`로 ROLLBACK 최적화
    /// - SSI 검증: Serializable 격리 수준에서 rw-antidependency 검사
    pub fn record_write(&self, tx_id: TransactionId, item: ReadSetItem) {
        // 모든 격리 수준에서 쓰기를 추적 (빈 트랜잭션 감지 + SSI 검증)
        self.write_sets
            .write()
            .entry(tx_id)
            .or_default()
            .insert(item);
    }

    /// 노드 읽기 기록 (편의 메서드)
    pub fn record_node_read(&self, tx_id: TransactionId, node_id: u64) {
        self.record_read(tx_id, ReadSetItem::Node(node_id));
    }

    /// 노드 쓰기 기록 (편의 메서드)
    pub fn record_node_write(&self, tx_id: TransactionId, node_id: u64) {
        self.record_write(tx_id, ReadSetItem::Node(node_id));
    }

    /// 엣지 읽기 기록 (편의 메서드)
    pub fn record_edge_read(&self, tx_id: TransactionId, edge_id: u64) {
        self.record_read(tx_id, ReadSetItem::Edge(edge_id));
    }

    /// 엣지 쓰기 기록 (편의 메서드)
    pub fn record_edge_write(&self, tx_id: TransactionId, edge_id: u64) {
        self.record_write(tx_id, ReadSetItem::Edge(edge_id));
    }

    /// SSI 검증 (Serializable용)
    ///
    /// 커밋 전 rw-antidependency 사이클을 검사합니다.
    /// 위험한 구조 (dangerous structure)가 감지되면 SerializationFailure 반환.
    ///
    /// # SSI 위험 구조
    /// T1이 읽은 데이터를 T2가 수정하고 (rw-dependency: T1 -> T2)
    /// T2가 읽은 데이터를 T1이 수정한 경우 (rw-dependency: T2 -> T1)
    /// 이 사이클은 직렬화 불가능을 의미합니다.
    pub fn validate_serializable(
        &self,
        tx_id: TransactionId,
    ) -> std::result::Result<(), ConflictError> {
        let snapshot_arc = match self.get_snapshot(tx_id) {
            Some(s) if s.isolation_level == IsolationLevel::Serializable => s,
            _ => return Ok(()), // Serializable이 아니면 검증 불필요
        };
        let snapshot = snapshot_arc.as_ref();

        let read_set = self
            .read_sets
            .read()
            .get(&tx_id)
            .cloned()
            .unwrap_or_default();
        let write_set = self
            .write_sets
            .read()
            .get(&tx_id)
            .cloned()
            .unwrap_or_default();

        // 현재 트랜잭션이 읽은 데이터를 최근 커밋된 트랜잭션이 수정했는지 확인
        // (rw-antidependency: 다른 TX가 우리가 읽은 것을 수정)
        let committed = self.committed_write_sets.read();
        for (&other_tx, other_writes) in committed.iter() {
            // 스냅샷 이후에 커밋된 트랜잭션만 검사
            if other_tx > snapshot.max_tx || snapshot.active_transactions.contains(&other_tx) {
                // 다른 TX가 우리가 읽은 데이터를 수정했는지
                if !read_set.is_disjoint(other_writes) {
                    // 우리도 다른 TX가 읽은 데이터를 수정했는지 확인 (사이클)
                    // 참고: 다른 TX의 읽기 세트는 이미 정리되었을 수 있으므로
                    // 보수적으로 쓰기 세트만 있으면 충돌로 간주
                    if !write_set.is_empty() && !write_set.is_disjoint(other_writes) {
                        return Err(ConflictError::SerializationFailure);
                    }
                }
            }
        }

        Ok(())
    }

    /// 읽기 세트 조회
    pub fn get_read_set(&self, tx_id: TransactionId) -> HashSet<ReadSetItem> {
        self.read_sets
            .read()
            .get(&tx_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 쓰기 세트 조회
    pub fn get_write_set(&self, tx_id: TransactionId) -> HashSet<ReadSetItem> {
        self.write_sets
            .read()
            .get(&tx_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 트랜잭션이 데이터를 수정했는지 확인 (빈 트랜잭션 조기 종료용)
    ///
    /// ROLLBACK 시 빈 트랜잭션(쓰기 작업 없음)은 WAL 스캔을 건너뛸 수 있습니다.
    /// 이 메서드는 O(1) 시간에 수정 여부를 확인합니다.
    ///
    /// # 반환값
    /// - `true`: 트랜잭션이 데이터를 수정함 (UNDO 필요)
    /// - `false`: 빈 트랜잭션 또는 알 수 없음 (UNDO 건너뛰기 가능)
    pub fn has_write_set(&self, tx_id: TransactionId) -> bool {
        self.write_sets
            .read()
            .get(&tx_id)
            .map(|set| !set.is_empty())
            .unwrap_or(false)
    }

    /// 활성 트랜잭션 목록 조회
    pub fn get_active_transactions(&self) -> HashSet<TransactionId> {
        self.active_transactions.read().clone()
    }

    /// 가비지 컬렉션 - 더 이상 필요 없는 버전 정리
    ///
    /// min_active_tx보다 작은 커밋 타임스탬프를 가진 삭제된 버전은 안전하게 제거 가능
    pub fn calculate_gc_watermark(&self) -> Option<u64> {
        let active = self.active_transactions.read();
        if active.is_empty() {
            // 활성 트랜잭션이 없으면 현재 타임스탬프까지 GC 가능
            Some(*self.next_timestamp.read())
        } else {
            // 가장 작은 활성 트랜잭션의 스냅샷 타임스탬프
            let snapshots = self.snapshots.read();
            active
                .iter()
                .filter_map(|tx| snapshots.get(tx).map(|s| s.timestamp))
                .min()
        }
    }

    /// 버전이 GC 대상인지 확인
    ///
    /// 다음 조건을 모두 만족하면 GC 가능:
    /// 1. 버전이 삭제됨 (deleted_by가 있음)
    /// 2. 삭제 트랜잭션이 커밋됨
    /// 3. 삭제 타임스탬프가 GC watermark보다 작음
    pub fn can_gc_version(&self, version: &VersionInfo) -> bool {
        // 삭제되지 않은 버전은 GC 불가
        let deleted_by = match version.deleted_by {
            Some(tx) => tx,
            None => return false,
        };

        // 삭제 트랜잭션이 커밋되지 않았으면 GC 불가
        if !self.is_committed(deleted_by) {
            return false;
        }

        // 삭제 타임스탬프 확인
        let deleted_ts = match version.deleted_ts {
            Some(ts) => ts,
            None => return false, // 타임스탬프가 없으면 GC 불가
        };

        // GC watermark와 비교
        match self.calculate_gc_watermark() {
            Some(watermark) => deleted_ts < watermark,
            None => false,
        }
    }

    /// 커밋 타임스탬프 GC - 오래된 커밋 정보 정리
    ///
    /// GC watermark보다 오래된 커밋 타임스탬프를 정리합니다.
    /// 반환값: 정리된 항목 수
    pub fn gc_commit_timestamps(&self) -> usize {
        let watermark = match self.calculate_gc_watermark() {
            Some(w) => w,
            None => return 0,
        };

        let mut commit_ts = self.commit_timestamps.write();
        let before_count = commit_ts.len();

        // watermark보다 오래된 커밋 타임스탬프 제거
        commit_ts.retain(|_, &mut ts| ts >= watermark);

        before_count - commit_ts.len()
    }

    /// MVCC 엔진 통계 정보
    pub fn stats(&self) -> MvccStats {
        MvccStats {
            active_transactions: self.active_transactions.read().len(),
            snapshots: self.snapshots.read().len(),
            commit_timestamps: self.commit_timestamps.read().len(),
            current_timestamp: *self.next_timestamp.read(),
            gc_watermark: self.calculate_gc_watermark(),
        }
    }
}

/// MVCC 엔진 통계 정보
#[derive(Debug, Clone)]
pub struct MvccStats {
    /// 활성 트랜잭션 수
    pub active_transactions: usize,
    /// 스냅샷 수
    pub snapshots: usize,
    /// 저장된 커밋 타임스탬프 수
    pub commit_timestamps: usize,
    /// 현재 타임스탬프
    pub current_timestamp: u64,
    /// GC watermark
    pub gc_watermark: Option<u64>,
}

impl Default for MvccEngine {
    fn default() -> Self {
        Self::new()
    }
}
