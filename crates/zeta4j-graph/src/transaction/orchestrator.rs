// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # 트랜잭션 오케스트레이터
//!
//! 트랜잭션의 전체 생명주기를 관리하고 모든 트랜잭션 관련 컴포넌트를 조정합니다.
//!
//! ## 구성요소
//!
//! | 컴포넌트 | 역할 | ACID 속성 |
//! |----------|------|-----------|
//! | [`TransactionManager`] | 트랜잭션 상태 관리 | Atomicity |
//! | [`LockManager`] | 동시성 제어 (2PL) | Isolation |
//! | [`PageWAL`] | 내구성 보장 | Durability |
//! | [`MvccEngine`] | 격리 수준 구현 | Consistency |
//!
//! ## 트랜잭션 생명주기
//!
//! ```text
//! begin() → 락 획득 → 데이터 조작 → commit()/rollback()
//!    ↓         ↓           ↓              ↓
//! TX ID 할당  2PL 적용   WAL 기록     락 해제
//! 스냅샷 생성             MVCC 추적   상태 업데이트
//! ```
//!
//! ## 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::transaction::{TransactionOrchestrator, TransactionConfig, IsolationLevel};
//!
//! let orchestrator = TransactionOrchestrator::new(
//!     manager, lock_manager, page_wal, mvcc
//! );
//!
//! // 트랜잭션 시작
//! let tx = orchestrator.begin()?;
//!
//! // 락 획득 및 데이터 조작
//! tx.acquire_node_lock(node_id, LockMode::Exclusive)?;
//! // ... 작업 수행 ...
//!
//! // 커밋 또는 롤백
//! tx.commit()?;
//! // 또는 tx.rollback()?;
//! ```

use std::sync::Arc;

use std::time::Instant;

use super::lock::{LockMode, LockResource};
use super::metrics::MetricsCollector;
use super::{
    IsolationLevel, LockManager, MvccEngine, PageWAL, Result, Snapshot, TransactionConfig,
    TransactionError, TransactionId, TransactionManager, TransactionState,
};
use crate::core::{EdgeId, NodeId};

/// 트랜잭션 오케스트레이터
///
/// 모든 트랜잭션 관련 컴포넌트를 통합하여 ACID 속성을 보장합니다.
///
/// # 주요 메서드
///
/// - [`begin`](Self::begin): 기본 설정으로 트랜잭션 시작
/// - [`begin_with_config`](Self::begin_with_config): 사용자 정의 설정으로 트랜잭션 시작
/// - [`with_config`](Self::with_config): 기본 설정 변경 (빌더 패턴)
///
/// # ACID 보장
///
/// | 속성 | 구현 방법 |
/// |------|-----------|
/// | Atomicity | 전체 커밋/롤백, WAL 복구 |
/// | Consistency | 스키마 검증, 제약조건 |
/// | Isolation | 2PL + MVCC |
/// | Durability | WAL 동기화 |
///
/// # 스레드 안전성
///
/// 모든 내부 컴포넌트가 `Arc`로 감싸져 있어 여러 스레드에서
/// 안전하게 공유할 수 있습니다.
pub struct TransactionOrchestrator {
    /// 트랜잭션 상태 관리
    manager: Arc<TransactionManager>,

    /// 락 관리 (2PL)
    lock_manager: Arc<LockManager>,

    /// Page-level WAL (Optional - 디스크 모드에서만 사용)
    page_wal: Option<Arc<PageWAL>>,

    /// MVCC 엔진
    mvcc: Arc<MvccEngine>,

    /// 기본 설정
    default_config: TransactionConfig,

    /// 메트릭 수집기
    metrics: Arc<MetricsCollector>,
}

impl TransactionOrchestrator {
    /// 새 오케스트레이터 생성
    pub fn new(
        manager: Arc<TransactionManager>,
        lock_manager: Arc<LockManager>,
        page_wal: Option<Arc<PageWAL>>,
        mvcc: Arc<MvccEngine>,
    ) -> Self {
        Self {
            manager,
            lock_manager,
            page_wal,
            mvcc,
            default_config: TransactionConfig::default(),
            metrics: Arc::new(MetricsCollector::new()),
        }
    }

    /// 메트릭 수집기 반환
    ///
    /// 트랜잭션 시스템의 성능 메트릭을 조회할 수 있습니다.
    ///
    /// # 예시
    ///
    /// ```rust,ignore
    /// let metrics = orchestrator.metrics().snapshot();
    /// println!("TPS: {:.2}", metrics.transactions_per_second());
    /// ```
    pub fn metrics(&self) -> &MetricsCollector {
        &self.metrics
    }

    /// 기본 설정 변경
    pub fn with_config(mut self, config: TransactionConfig) -> Self {
        self.default_config = config;
        self
    }

    /// 새 트랜잭션 시작
    ///
    /// 1. TransactionManager에서 새 TX ID 할당
    /// 2. WAL에 BEGIN 레코드 기록
    /// 3. MVCC 스냅샷 생성
    pub fn begin(&self) -> Result<Transaction<'_>> {
        self.begin_with_config(self.default_config.clone())
    }

    /// 설정을 지정하여 트랜잭션 시작
    pub fn begin_with_config(&self, config: TransactionConfig) -> Result<Transaction<'_>> {
        // 메트릭: 트랜잭션 시작 기록
        self.metrics.record_begin();
        self.metrics.record_snapshot_created();

        // 1. 새 트랜잭션 시작
        let tx_id = self.manager.begin_transaction()?;

        // 2. WAL에 BEGIN 기록 (디스크 모드)
        if let Some(ref wal) = self.page_wal {
            wal.log_begin(tx_id)?;
        }

        // 3. MVCC 스냅샷 생성 (격리 수준에 따라)
        // - ReadCommitted: 스냅샷 생성하되, 각 읽기마다 갱신
        // - RepeatableRead: 시작 시점 스냅샷 고정
        // - Serializable: 스냅샷 + 읽기/쓰기 추적
        let snapshot = self
            .mvcc
            .begin_transaction_with_isolation(tx_id, config.isolation_level);

        Ok(Transaction {
            id: tx_id,
            orchestrator: self,
            config,
            snapshot: Some(snapshot),
            lock_timeout_ms: self.default_config.timeout_ms,
            start_time: Instant::now(),
        })
    }

    /// 트랜잭션 커밋
    ///
    /// 1. 상태 확인
    /// 2. Serializable SSI 검증
    /// 3. WAL에 COMMIT 레코드 기록 및 동기화
    /// 4. 커밋 타임스탬프 기록
    /// 5. 락 해제
    /// 6. 트랜잭션 상태 업데이트
    fn commit_transaction(&self, tx_id: TransactionId) -> Result<()> {
        // 1. 상태 확인
        let state = self.manager.get_transaction_state(tx_id)?;
        if state != TransactionState::Active {
            return Err(TransactionError::SystemError(format!(
                "Cannot commit transaction in state {:?}",
                state
            )));
        }

        // 2. Serializable SSI 검증 (rw-antidependency 사이클 감지)
        self.mvcc
            .validate_serializable(tx_id)
            .map_err(TransactionError::MvccConflict)?;

        // 3. WAL에 COMMIT 기록 (동기화)
        if let Some(ref wal) = self.page_wal {
            wal.log_commit(tx_id)?;
            wal.flush()?;
        }

        // 4. MVCC 커밋 타임스탬프 기록
        self.mvcc.commit_transaction(tx_id);

        // 5. 락 해제
        self.lock_manager.release_all_locks(tx_id);

        // 6. 트랜잭션 상태 업데이트
        self.manager.commit_transaction(tx_id)?;

        Ok(())
    }

    /// 트랜잭션 롤백
    ///
    /// 1. 락 해제 (다른 트랜잭션 블로킹 방지)
    /// 2. MVCC 롤백 처리
    /// 3. 트랜잭션 상태 업데이트
    /// 4. WAL에 ABORT 레코드 기록 (에러 시 로깅만)
    ///
    /// # 주의
    ///
    /// 락 해제를 가장 먼저 수행하여 WAL 에러가 발생해도 다른 트랜잭션이
    /// 무한 대기하지 않도록 합니다. 롤백은 "아무것도 하지 않은 것"과
    /// 동일하므로 WAL 기록 실패해도 데이터 무결성에 영향 없습니다.
    /// ABORT 레코드가 없으면 복구 시 해당 트랜잭션을 자동으로 롤백 처리합니다.
    fn rollback_transaction(&self, tx_id: TransactionId) -> Result<()> {
        // 상태 확인
        let state = self.manager.get_transaction_state(tx_id)?;
        if state == TransactionState::Committed {
            return Err(TransactionError::AlreadyCommitted(tx_id));
        }
        if state == TransactionState::Aborted {
            return Err(TransactionError::AlreadyAborted(tx_id));
        }

        // 1. 락 해제 먼저 (다른 트랜잭션 블로킹 방지)
        self.lock_manager.release_all_locks(tx_id);

        // 2. MVCC 롤백 처리
        self.mvcc.rollback_transaction(tx_id);

        // 3. 트랜잭션 상태 업데이트
        self.manager.rollback_transaction(tx_id)?;

        // 4. WAL에 ABORT 기록 (마지막에, 에러 시 로깅만)
        if let Some(ref wal) = self.page_wal
            && let Err(e) = wal.log_abort(tx_id)
        {
            tracing::error!(
                tx_id = tx_id,
                error = %e,
                "Failed to write WAL ABORT record - retry needed during recovery"
            );
        }

        Ok(())
    }

    /// Idempotently abort a transaction selected as a deadlock victim.
    fn abort_transaction(&self, tx_id: TransactionId) -> Result<()> {
        let aborted = self.manager.abort_if_active(tx_id)?;
        self.lock_manager.release_all_locks(tx_id);
        self.mvcc.rollback_transaction(tx_id);

        if aborted {
            if let Some(ref wal) = self.page_wal
                && let Err(error) = wal.log_abort(tx_id)
            {
                tracing::error!(
                    tx_id = tx_id,
                    error = %error,
                    "Failed to write WAL ABORT record for deadlock victim"
                );
            }
            self.metrics.record_abort(0);
            self.metrics.record_snapshot_released();
        }
        Ok(())
    }

    async fn acquire_lock(
        &self,
        tx_id: TransactionId,
        resource: LockResource,
        mode: LockMode,
        timeout_ms: u64,
    ) -> Result<()> {
        match self
            .lock_manager
            .acquire_lock_with_timeout(tx_id, resource, mode, timeout_ms)
            .await
        {
            Ok(()) => Ok(()),
            Err(error @ TransactionError::DeadlockDetected(_)) => {
                self.abort_transaction(tx_id)?;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    /// 노드 락 획득
    async fn acquire_node_lock(
        &self,
        tx_id: TransactionId,
        node_id: NodeId,
        mode: LockMode,
        timeout_ms: u64,
    ) -> Result<()> {
        self.acquire_lock(tx_id, LockResource::Node(node_id), mode, timeout_ms)
            .await
    }

    /// 엣지 락 획득
    async fn acquire_edge_lock(
        &self,
        tx_id: TransactionId,
        edge_id: EdgeId,
        mode: LockMode,
        timeout_ms: u64,
    ) -> Result<()> {
        self.acquire_lock(tx_id, LockResource::Edge(edge_id), mode, timeout_ms)
            .await
    }

    /// 데이터베이스 글로벌 락 획득
    async fn acquire_database_lock(
        &self,
        tx_id: TransactionId,
        mode: LockMode,
        timeout_ms: u64,
    ) -> Result<()> {
        self.acquire_lock(tx_id, LockResource::Database, mode, timeout_ms)
            .await
    }
}

/// 활성 트랜잭션 핸들
///
/// RAII 패턴으로 트랜잭션 생명주기 관리
pub struct Transaction<'a> {
    id: TransactionId,
    orchestrator: &'a TransactionOrchestrator,
    config: TransactionConfig,
    snapshot: Option<Snapshot>,
    lock_timeout_ms: u64,
    /// 트랜잭션 시작 시간 (메트릭용)
    start_time: Instant,
}

impl<'a> Transaction<'a> {
    /// 트랜잭션 ID 반환
    pub fn id(&self) -> TransactionId {
        self.id
    }

    /// 격리 수준 반환
    pub fn isolation_level(&self) -> IsolationLevel {
        self.config.isolation_level
    }

    /// 스냅샷 반환 (RepeatableRead 이상)
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// 노드 읽기 락 획득
    pub async fn read_node(&self, node_id: NodeId) -> Result<()> {
        self.orchestrator
            .acquire_node_lock(self.id, node_id, LockMode::Shared, self.lock_timeout_ms)
            .await
    }

    /// 노드 쓰기 락 획득
    pub async fn write_node(&self, node_id: NodeId) -> Result<()> {
        self.orchestrator
            .acquire_node_lock(self.id, node_id, LockMode::Exclusive, self.lock_timeout_ms)
            .await
    }

    /// 엣지 읽기 락 획득
    pub async fn read_edge(&self, edge_id: EdgeId) -> Result<()> {
        self.orchestrator
            .acquire_edge_lock(self.id, edge_id, LockMode::Shared, self.lock_timeout_ms)
            .await
    }

    /// 엣지 쓰기 락 획득
    pub async fn write_edge(&self, edge_id: EdgeId) -> Result<()> {
        self.orchestrator
            .acquire_edge_lock(self.id, edge_id, LockMode::Exclusive, self.lock_timeout_ms)
            .await
    }

    /// 데이터베이스 글로벌 읽기 락
    pub async fn read_database(&self) -> Result<()> {
        self.orchestrator
            .acquire_database_lock(self.id, LockMode::Shared, self.lock_timeout_ms)
            .await
    }

    /// 데이터베이스 글로벌 쓰기 락
    pub async fn write_database(&self) -> Result<()> {
        self.orchestrator
            .acquire_database_lock(self.id, LockMode::Exclusive, self.lock_timeout_ms)
            .await
    }

    /// 트랜잭션 커밋
    pub fn commit(self) -> Result<()> {
        let elapsed_ms = self.start_time.elapsed().as_millis() as u64;
        let result = self.orchestrator.commit_transaction(self.id);
        if result.is_ok() {
            self.orchestrator.metrics.record_commit(elapsed_ms);
            self.orchestrator.metrics.record_snapshot_released();
        }
        result
    }

    /// 트랜잭션 롤백
    pub fn rollback(self) -> Result<()> {
        let elapsed_ms = self.start_time.elapsed().as_millis() as u64;
        let result = self.orchestrator.rollback_transaction(self.id);
        if result.is_ok() {
            self.orchestrator.metrics.record_abort(elapsed_ms);
            self.orchestrator.metrics.record_snapshot_released();
        }
        result
    }
}

/// 트랜잭션이 Drop될 때 자동으로 롤백 (RAII)
impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        // 아직 활성 상태이면 롤백 시도
        if let Ok(state) = self.orchestrator.manager.get_transaction_state(self.id)
            && state == TransactionState::Active
        {
            // 롤백 시도 (실패시 로깅)
            if let Err(e) = self.orchestrator.rollback_transaction(self.id) {
                tracing::warn!(
                    tx_id = self.id,
                    error = %e,
                    "Rollback failed during transaction Drop"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn orchestrator_aborts_cycle_closing_victim_before_return()
    -> std::result::Result<(), TransactionError> {
        let manager = Arc::new(TransactionManager::new());
        let locks = Arc::new(LockManager::new());
        let mvcc = Arc::new(MvccEngine::new());
        let orchestrator =
            TransactionOrchestrator::new(Arc::clone(&manager), Arc::clone(&locks), None, mvcc);
        let first = orchestrator.begin()?;
        let victim = orchestrator.begin()?;
        first.write_node(1).await?;
        victim.write_node(2).await?;

        let first_wait = first.write_node(2);
        let close_cycle = async {
            tokio::time::timeout(Duration::from_secs(2), async {
                while locks.waiter_count() != 1 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(|_| TransactionError::SystemError("waiter was not registered".to_string()))?;
            victim.write_node(1).await
        };
        let (first_result, victim_result) = tokio::join!(first_wait, close_cycle);

        first_result?;
        assert!(matches!(
            victim_result,
            Err(TransactionError::DeadlockDetected(_))
        ));
        assert!(manager.get_transaction_context(victim.id()).is_none());
        assert_eq!(locks.locks_held_by(victim.id()), 0);
        assert_eq!(locks.waiter_count(), 0);
        first.rollback()?;
        Ok(())
    }
}
