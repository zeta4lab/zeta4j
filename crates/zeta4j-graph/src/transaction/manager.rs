//! # 트랜잭션 매니저
//!
//! 트랜잭션의 생명주기를 관리하는 핵심 컴포넌트입니다.
//!
//! ## 주요 기능
//!
//! - 트랜잭션 시작/커밋/롤백
//! - 트랜잭션 타임아웃 관리
//! - 활성 트랜잭션 모니터링
//! - 통계 수집
//!
//! ## 스레드 안전성
//!
//! `TransactionManager`는 `Send + Sync`이며, 여러 스레드에서
//! 동시에 안전하게 사용할 수 있습니다.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use super::{IsolationLevel, Result, TransactionError, TransactionId};

/// 트랜잭션 상태
///
/// 트랜잭션의 현재 상태를 나타냅니다.
///
/// ## 상태 전이
///
/// ```text
/// Active → Committing → Committed
///    ↓
/// Aborting → Aborted
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransactionState {
    /// 활성 상태 - 작업 수행 가능
    Active,
    /// 커밋 진행 중
    Committing,
    /// 커밋 완료
    Committed,
    /// 롤백 진행 중
    Aborting,
    /// 롤백 완료
    Aborted,
}

/// 트랜잭션 컨텍스트
///
/// 개별 트랜잭션의 메타데이터를 저장합니다.
///
/// # 필드
///
/// - `id`: 트랜잭션 고유 식별자
/// - `state`: 현재 상태 ([`TransactionState`])
/// - `start_time`: 시작 시간
/// - `timeout_ms`: 타임아웃 설정 (밀리초, 0이면 무제한)
///
/// # 예시
///
/// ```rust,ignore
/// use zeta4j_graph::transaction::{TransactionContext, TransactionState};
///
/// // 기본 생성 (무제한 타임아웃)
/// let ctx = TransactionContext::new(1);
/// assert_eq!(ctx.state, TransactionState::Active);
/// assert!(!ctx.is_timed_out());
///
/// // 타임아웃 설정과 함께 생성
/// let ctx = TransactionContext::with_timeout(2, 5000); // 5초 타임아웃
/// assert!(ctx.remaining_ms().is_some());
/// ```
#[derive(Debug, Clone)]
pub struct TransactionContext {
    pub id: TransactionId,
    pub state: TransactionState,
    pub start_time: Instant,
    /// 트랜잭션 타임아웃 (밀리초, 0이면 무제한)
    pub timeout_ms: u64,
    /// 트랜잭션 격리 수준
    pub isolation_level: IsolationLevel,
    /// 실행된 문장 수
    pub statements_executed: u64,
    /// 마지막 활동 시간
    pub last_activity: Instant,
}

/// Single-use ownership of a transaction already moved to `Committing`.
///
/// Holding the context directly makes the terminal transition independent of
/// a second fallible map lookup after canonical persistence has committed.
#[derive(Debug)]
pub(crate) struct PreparedTransactionCommit {
    tx_id: TransactionId,
    context: Arc<RwLock<TransactionContext>>,
}

impl PreparedTransactionCommit {
    pub(crate) const fn transaction_id(&self) -> TransactionId {
        self.tx_id
    }
}

impl TransactionContext {
    pub fn new(id: TransactionId) -> Self {
        let now = Instant::now();
        Self {
            id,
            state: TransactionState::Active,
            start_time: now,
            timeout_ms: 0, // 기본값: 무제한
            isolation_level: IsolationLevel::ReadCommitted,
            statements_executed: 0,
            last_activity: now,
        }
    }

    /// 타임아웃 설정과 함께 생성
    pub fn with_timeout(id: TransactionId, timeout_ms: u64) -> Self {
        let now = Instant::now();
        Self {
            id,
            state: TransactionState::Active,
            start_time: now,
            timeout_ms,
            isolation_level: IsolationLevel::ReadCommitted,
            statements_executed: 0,
            last_activity: now,
        }
    }

    /// 격리 수준과 타임아웃 설정과 함께 생성
    pub fn with_config(
        id: TransactionId,
        isolation_level: IsolationLevel,
        timeout_ms: u64,
    ) -> Self {
        let now = Instant::now();
        Self {
            id,
            state: TransactionState::Active,
            start_time: now,
            timeout_ms,
            isolation_level,
            statements_executed: 0,
            last_activity: now,
        }
    }

    /// 문장 실행 기록
    pub fn record_statement(&mut self) {
        self.statements_executed += 1;
        self.last_activity = Instant::now();
    }

    /// 마지막 활동 이후 경과 시간 (밀리초)
    pub fn idle_time_ms(&self) -> u64 {
        self.last_activity.elapsed().as_millis() as u64
    }

    /// 트랜잭션이 타임아웃되었는지 확인
    pub fn is_timed_out(&self) -> bool {
        if self.timeout_ms == 0 {
            return false; // 무제한
        }
        self.start_time.elapsed().as_millis() as u64 > self.timeout_ms
    }

    /// 트랜잭션 경과 시간 (밀리초)
    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    /// 남은 시간 (밀리초), 무제한이면 None
    pub fn remaining_ms(&self) -> Option<u64> {
        if self.timeout_ms == 0 {
            return None;
        }
        let elapsed = self.elapsed_ms();
        if elapsed >= self.timeout_ms {
            Some(0)
        } else {
            Some(self.timeout_ms - elapsed)
        }
    }
}

/// 트랜잭션 매니저
///
/// 트랜잭션의 전체 생명주기를 관리하는 핵심 컴포넌트입니다.
///
/// # 주요 기능
///
/// - **트랜잭션 시작**: [`begin_transaction`](Self::begin_transaction),
///   [`begin_transaction_with_timeout`](Self::begin_transaction_with_timeout)
/// - **트랜잭션 종료**: [`commit_transaction`](Self::commit_transaction),
///   [`rollback_transaction`](Self::rollback_transaction)
/// - **상태 조회**: [`get_transaction_state`](Self::get_transaction_state),
///   [`get_transaction_context`](Self::get_transaction_context)
/// - **타임아웃 관리**: [`is_transaction_timed_out`](Self::is_transaction_timed_out),
///   [`abort_timed_out_transactions`](Self::abort_timed_out_transactions)
/// - **통계**: [`stats`](Self::stats), [`active_transaction_count`](Self::active_transaction_count)
///
/// # 스레드 안전성
///
/// `TransactionManager`는 `Send + Sync`를 구현하며, 여러 스레드에서
/// 동시에 안전하게 사용할 수 있습니다.
///
/// # 예시
///
/// ```rust,ignore
/// use zeta4j_graph::transaction::TransactionManager;
///
/// let manager = TransactionManager::new();
///
/// // 트랜잭션 시작
/// let tx_id = manager.begin_transaction()?;
///
/// // 작업 수행...
///
/// // 커밋 또는 롤백
/// manager.commit_transaction(tx_id)?;
/// ```
///
/// # 타임아웃 예시
///
/// ```rust,ignore
/// use std::time::Duration;
/// use std::thread;
///
/// let manager = TransactionManager::new();
///
/// // 100ms 타임아웃 설정
/// let tx_id = manager.begin_transaction_with_timeout(100)?;
///
/// thread::sleep(Duration::from_millis(150));
///
/// // 타임아웃 체크
/// assert!(manager.is_transaction_timed_out(tx_id)?);
///
/// // 타임아웃된 트랜잭션 자동 중단
/// let aborted = manager.abort_timed_out_transactions();
/// assert!(aborted.contains(&tx_id));
/// ```
pub struct TransactionManager {
    /// 다음 트랜잭션 ID
    next_tx_id: AtomicU64,

    /// 시작된 트랜잭션 누계
    started_transactions: AtomicUsize,

    /// 커밋된 트랜잭션 누계
    committed_transactions: AtomicUsize,

    /// 중단된 트랜잭션 누계
    aborted_transactions: AtomicUsize,

    /// 활성 트랜잭션 목록 (Arc<RwLock>으로 공유 가변 상태 지원)
    active_transactions: RwLock<HashMap<TransactionId, Arc<RwLock<TransactionContext>>>>,
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            next_tx_id: AtomicU64::new(1),
            started_transactions: AtomicUsize::new(0),
            committed_transactions: AtomicUsize::new(0),
            aborted_transactions: AtomicUsize::new(0),
            active_transactions: RwLock::new(HashMap::new()),
        }
    }

    /// 새 트랜잭션 시작
    pub fn begin_transaction(&self) -> Result<TransactionId> {
        let tx_id = self.next_tx_id.fetch_add(1, Ordering::SeqCst);
        let context = Arc::new(RwLock::new(TransactionContext::new(tx_id)));

        let mut transactions = self.active_transactions.write();
        transactions.insert(tx_id, context);
        self.started_transactions.fetch_add(1, Ordering::Relaxed);

        Ok(tx_id)
    }

    /// 격리 수준과 타임아웃을 지정하여 트랜잭션 시작
    pub fn begin_transaction_with_config(
        &self,
        isolation_level: IsolationLevel,
        timeout_ms: u64,
    ) -> Result<TransactionId> {
        let tx_id = self.next_tx_id.fetch_add(1, Ordering::SeqCst);
        let context = Arc::new(RwLock::new(TransactionContext::with_config(
            tx_id,
            isolation_level,
            timeout_ms,
        )));

        let mut transactions = self.active_transactions.write();
        transactions.insert(tx_id, context);
        self.started_transactions.fetch_add(1, Ordering::Relaxed);

        Ok(tx_id)
    }

    /// 트랜잭션 격리 수준 조회
    pub fn get_isolation_level(&self, tx_id: TransactionId) -> Result<IsolationLevel> {
        let transactions = self.active_transactions.read();
        match transactions.get(&tx_id) {
            Some(ctx_arc) => Ok(ctx_arc.read().isolation_level),
            None => Err(TransactionError::NotFound(tx_id)),
        }
    }

    /// 문장 실행 기록
    pub fn record_statement(&self, tx_id: TransactionId) -> Result<()> {
        let transactions = self.active_transactions.read();
        match transactions.get(&tx_id) {
            Some(ctx_arc) => {
                ctx_arc.write().record_statement();
                Ok(())
            }
            None => Err(TransactionError::NotFound(tx_id)),
        }
    }

    /// 트랜잭션 상태 조회
    pub fn get_transaction_state(&self, tx_id: TransactionId) -> Result<TransactionState> {
        let transactions = self.active_transactions.read();

        match transactions.get(&tx_id) {
            Some(ctx_arc) => Ok(ctx_arc.read().state),
            None => Err(TransactionError::NotFound(tx_id)),
        }
    }

    /// 트랜잭션 커밋
    pub fn commit_transaction(&self, tx_id: TransactionId) -> Result<()> {
        let prepared = self.begin_commit(tx_id)?;
        self.complete_commit(prepared);
        Ok(())
    }

    /// Claim an active transaction for a single commit coordinator.
    pub(crate) fn begin_commit(&self, tx_id: TransactionId) -> Result<PreparedTransactionCommit> {
        let transactions = self.active_transactions.read();

        let ctx_arc = transactions
            .get(&tx_id)
            .cloned()
            .ok_or(TransactionError::NotFound(tx_id))?;
        let mut ctx = ctx_arc.write();

        match ctx.state {
            TransactionState::Active => {
                ctx.state = TransactionState::Committing;
                drop(ctx);
                Ok(PreparedTransactionCommit {
                    tx_id,
                    context: ctx_arc,
                })
            }
            TransactionState::Committed => Err(TransactionError::AlreadyCommitted(tx_id)),
            TransactionState::Aborted => Err(TransactionError::AlreadyAborted(tx_id)),
            _ => Err(TransactionError::invalid_state(
                tx_id,
                &format!("{:?}", ctx.state),
                "Active",
            )),
        }
    }

    /// Finish a transaction previously claimed by [`Self::begin_commit`].
    pub(crate) fn complete_commit(&self, prepared: PreparedTransactionCommit) {
        let mut transactions = self.active_transactions.write();
        prepared.context.write().state = TransactionState::Committed;
        transactions.remove(&prepared.tx_id);
        self.committed_transactions.fetch_add(1, Ordering::Relaxed);
    }

    /// Abort a transaction whose external canonical commit failed.
    pub(crate) fn abort_commit(&self, prepared: PreparedTransactionCommit) {
        let mut transactions = self.active_transactions.write();
        prepared.context.write().state = TransactionState::Aborted;
        transactions.remove(&prepared.tx_id);
        self.aborted_transactions.fetch_add(1, Ordering::Relaxed);
    }

    /// 트랜잭션 롤백
    pub fn rollback_transaction(&self, tx_id: TransactionId) -> Result<()> {
        let mut transactions = self.active_transactions.write();

        let ctx_arc = transactions
            .get(&tx_id)
            .cloned()
            .ok_or(TransactionError::NotFound(tx_id))?;
        let mut ctx = ctx_arc.write();

        match ctx.state {
            TransactionState::Active => {
                ctx.state = TransactionState::Aborted;
                drop(ctx);
                transactions.remove(&tx_id);
                self.aborted_transactions.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            TransactionState::Committed => Err(TransactionError::AlreadyCommitted(tx_id)),
            TransactionState::Aborted => Err(TransactionError::AlreadyAborted(tx_id)),
            _ => Err(TransactionError::invalid_state(
                tx_id,
                &format!("{:?}", ctx.state),
                "Active",
            )),
        }
    }

    /// Abort an active transaction if it still exists.
    ///
    /// This is the idempotent terminal transition used by asynchronous failure
    /// paths such as deadlock victim selection. A protocol layer may observe
    /// the same failure and request cleanup again, so an already-removed
    /// transaction is treated as successfully cleaned up.
    pub(crate) fn abort_if_active(&self, tx_id: TransactionId) -> Result<bool> {
        let mut transactions = self.active_transactions.write();
        let Some(ctx_arc) = transactions.get(&tx_id).cloned() else {
            return Ok(false);
        };
        let mut ctx = ctx_arc.write();

        match ctx.state {
            TransactionState::Active => {
                ctx.state = TransactionState::Aborted;
                drop(ctx);
                transactions.remove(&tx_id);
                self.aborted_transactions.fetch_add(1, Ordering::Relaxed);
                Ok(true)
            }
            TransactionState::Aborted => {
                drop(ctx);
                transactions.remove(&tx_id);
                Ok(false)
            }
            TransactionState::Committed => Err(TransactionError::AlreadyCommitted(tx_id)),
            _ => Err(TransactionError::invalid_state(
                tx_id,
                &format!("{:?}", ctx.state),
                "Active",
            )),
        }
    }

    /// 타임아웃 설정과 함께 트랜잭션 시작
    pub fn begin_transaction_with_timeout(&self, timeout_ms: u64) -> Result<TransactionId> {
        let tx_id = self.next_tx_id.fetch_add(1, Ordering::SeqCst);
        let context = Arc::new(RwLock::new(TransactionContext::with_timeout(
            tx_id, timeout_ms,
        )));

        let mut transactions = self.active_transactions.write();
        transactions.insert(tx_id, context);
        self.started_transactions.fetch_add(1, Ordering::Relaxed);

        Ok(tx_id)
    }

    /// 특정 트랜잭션이 타임아웃되었는지 확인
    pub fn is_transaction_timed_out(&self, tx_id: TransactionId) -> Result<bool> {
        let transactions = self.active_transactions.read();
        match transactions.get(&tx_id) {
            Some(ctx_arc) => Ok(ctx_arc.read().is_timed_out()),
            None => Err(TransactionError::NotFound(tx_id)),
        }
    }

    /// 타임아웃된 모든 트랜잭션 ID 조회
    pub fn get_timed_out_transactions(&self) -> Vec<TransactionId> {
        let transactions = self.active_transactions.read();
        transactions
            .iter()
            .filter(|(_, ctx_arc)| {
                let ctx = ctx_arc.read();
                ctx.state == TransactionState::Active && ctx.is_timed_out()
            })
            .map(|(&id, _)| id)
            .collect()
    }

    /// 타임아웃된 트랜잭션 자동 중단
    ///
    /// 타임아웃된 모든 활성 트랜잭션을 중단하고 활성 목록에서 제거합니다.
    /// 반환값: 중단된 트랜잭션 ID 목록
    pub fn abort_timed_out_transactions(&self) -> Vec<TransactionId> {
        let mut transactions = self.active_transactions.write();
        let mut aborted = Vec::new();

        for (&id, ctx_arc) in transactions.iter() {
            let mut ctx = ctx_arc.write();
            if ctx.state == TransactionState::Active && ctx.is_timed_out() {
                ctx.state = TransactionState::Aborted;
                aborted.push(id);
            }
        }

        for tx_id in &aborted {
            transactions.remove(tx_id);
        }
        self.aborted_transactions
            .fetch_add(aborted.len(), Ordering::Relaxed);

        aborted
    }

    /// 트랜잭션 컨텍스트 조회 (Arc 참조 반환으로 clone 비용 절감)
    pub fn get_transaction_context(
        &self,
        tx_id: TransactionId,
    ) -> Option<Arc<RwLock<TransactionContext>>> {
        let transactions = self.active_transactions.read();
        transactions.get(&tx_id).cloned()
    }

    /// 활성 트랜잭션 수 조회
    pub fn active_transaction_count(&self) -> usize {
        let transactions = self.active_transactions.read();
        transactions
            .values()
            .filter(|ctx_arc| ctx_arc.read().state == TransactionState::Active)
            .count()
    }

    /// 활성 트랜잭션 ID 목록 조회
    ///
    /// 현재 Active 상태인 모든 트랜잭션의 ID를 반환합니다.
    pub fn active_transactions(&self) -> Vec<TransactionId> {
        let transactions = self.active_transactions.read();
        transactions
            .iter()
            .filter(|(_, ctx_arc)| ctx_arc.read().state == TransactionState::Active)
            .map(|(&id, _)| id)
            .collect()
    }

    /// 트랜잭션 매니저 통계
    pub fn stats(&self) -> TransactionManagerStats {
        let transactions = self.active_transactions.read();
        let active = transactions
            .values()
            .filter(|ctx_arc| ctx_arc.read().state == TransactionState::Active)
            .count();
        let timed_out = transactions
            .values()
            .filter(|ctx_arc| {
                let ctx = ctx_arc.read();
                ctx.state == TransactionState::Active && ctx.is_timed_out()
            })
            .count();

        TransactionManagerStats {
            total: self.started_transactions.load(Ordering::Relaxed),
            active,
            timed_out,
            committed: self.committed_transactions.load(Ordering::Relaxed),
            aborted: self.aborted_transactions.load(Ordering::Relaxed),
            next_tx_id: self.next_tx_id.load(Ordering::SeqCst),
        }
    }
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 트랜잭션 매니저 통계 정보
///
/// [`TransactionManager::stats`]에서 반환되는 통계 스냅샷입니다.
///
/// # 필드
///
/// | 필드 | 설명 |
/// |------|------|
/// | `total` | 시작된 전체 트랜잭션 누계 |
/// | `active` | 현재 활성(Active) 상태인 트랜잭션 수 |
/// | `timed_out` | 활성 트랜잭션 중 타임아웃된 수 |
/// | `committed` | 커밋 완료된 트랜잭션 누계 |
/// | `aborted` | 중단된 트랜잭션 누계 |
/// | `next_tx_id` | 다음에 발급될 트랜잭션 ID |
///
/// # 예시
///
/// ```rust,ignore
/// let manager = TransactionManager::new();
/// let tx1 = manager.begin_transaction()?;
/// let tx2 = manager.begin_transaction()?;
/// manager.commit_transaction(tx1)?;
///
/// let stats = manager.stats();
/// assert_eq!(stats.total, 2);
/// assert_eq!(stats.active, 1);
/// assert_eq!(stats.committed, 1);
/// ```
#[derive(Debug, Clone)]
pub struct TransactionManagerStats {
    /// 시작된 전체 트랜잭션 누계
    pub total: usize,
    /// 활성 트랜잭션 수
    pub active: usize,
    /// 타임아웃된 활성 트랜잭션 수
    pub timed_out: usize,
    /// 커밋된 트랜잭션 누계
    pub committed: usize,
    /// 중단된 트랜잭션 누계
    pub aborted: usize,
    /// 다음 트랜잭션 ID
    pub next_tx_id: u64,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::TransactionManager;

    #[test]
    fn terminal_transactions_are_removed_and_counted()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let manager = TransactionManager::new();

        let committed = manager.begin_transaction()?;
        manager.commit_transaction(committed)?;

        let rolled_back = manager.begin_transaction()?;
        manager.rollback_transaction(rolled_back)?;

        let failed_commit = manager.begin_transaction()?;
        let prepared = manager.begin_commit(failed_commit)?;
        manager.abort_commit(prepared);

        assert!(manager.get_transaction_context(committed).is_none());
        assert!(manager.get_transaction_context(rolled_back).is_none());
        assert!(manager.get_transaction_context(failed_commit).is_none());

        let stats = manager.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.active, 0);
        assert_eq!(stats.committed, 1);
        assert_eq!(stats.aborted, 2);
        Ok(())
    }

    #[test]
    fn timed_out_transactions_are_removed_and_counted()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let manager = TransactionManager::new();
        let tx_id = manager.begin_transaction_with_timeout(1)?;
        let context = manager
            .get_transaction_context(tx_id)
            .ok_or_else(|| std::io::Error::other("transaction context must exist"))?;
        context.write().start_time -= Duration::from_millis(2);

        assert_eq!(manager.abort_timed_out_transactions(), vec![tx_id]);
        assert!(manager.get_transaction_context(tx_id).is_none());

        let stats = manager.stats();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.active, 0);
        assert_eq!(stats.aborted, 1);
        assert_eq!(stats.timed_out, 0);
        Ok(())
    }
}
