//! 배치 트랜잭션 API
//!
//! 대량 쓰기 작업을 단일 트랜잭션으로 배치 처리하여
//! 커밋 오버헤드를 줄이고 페이지 플러시 빈도를 감소시킵니다.
//!
//! ## 문제점
//!
//! 쓰기마다 개별 트랜잭션으로 처리하면:
//! - 쓰기당 커밋 오버헤드 누적
//! - 페이지 플러시 빈번 발생
//! - 버퍼 풀 압박
//!
//! ## 해결책
//!
//! `BatchTransaction`은 여러 쓰기 작업을 모아서:
//! - 배치 크기에 도달하거나 명시적 flush 시 한 번에 커밋
//! - WAL 그룹 커밋과 통합
//! - 실패 시 전체 배치 롤백

use std::sync::Arc;

use crate::error::{GraphError, Result};
use crate::transaction::{TransactionId, TransactionManager};

/// 배치 트랜잭션 설정
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// 배치 크기 (이 개수 도달 시 자동 flush)
    pub batch_size: usize,
    /// 자동 flush 활성화
    pub auto_flush: bool,
    /// 실패 시 전체 롤백 (false면 개별 실패 무시)
    pub rollback_on_error: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            auto_flush: true,
            rollback_on_error: true,
        }
    }
}

/// 배치 쓰기 작업
pub trait BatchWrite: Send {
    /// 작업 실행
    fn execute(&self, tx_id: TransactionId) -> Result<()>;

    /// 작업 설명 (디버깅용)
    fn description(&self) -> String;
}

/// 클로저 기반 BatchWrite 구현
pub struct BatchWriteFn<F> {
    func: F,
    desc: String,
}

impl<F> BatchWriteFn<F>
where
    F: Fn(TransactionId) -> Result<()> + Send,
{
    pub fn new(desc: impl Into<String>, func: F) -> Self {
        Self {
            func,
            desc: desc.into(),
        }
    }
}

impl<F> BatchWrite for BatchWriteFn<F>
where
    F: Fn(TransactionId) -> Result<()> + Send,
{
    fn execute(&self, tx_id: TransactionId) -> Result<()> {
        (self.func)(tx_id)
    }

    fn description(&self) -> String {
        self.desc.clone()
    }
}

/// 배치 트랜잭션 통계
#[derive(Debug, Clone, Default)]
pub struct BatchStats {
    /// 총 처리된 작업 수
    pub total_operations: usize,
    /// 성공한 작업 수
    pub successful_operations: usize,
    /// 실패한 작업 수
    pub failed_operations: usize,
    /// 수행된 배치 커밋 수
    pub batch_commits: usize,
    /// 롤백 수
    pub rollbacks: usize,
}

/// 배치 트랜잭션
///
/// 여러 쓰기 작업을 모아서 한 번에 커밋합니다.
///
/// # 사용 예시
///
/// ```ignore
/// let mut batch = BatchTransaction::new(tx_manager, BatchConfig::default());
///
/// for doc in documents {
///     batch.add(BatchWriteFn::new("index_doc", move |tx_id| {
///         // 문서 인덱싱 로직
///         Ok(())
///     }))?;
/// }
///
/// let stats = batch.commit()?;
/// ```
pub struct BatchTransaction<T: TransactionProvider> {
    provider: Arc<T>,
    config: BatchConfig,
    pending_writes: Vec<Box<dyn BatchWrite>>,
    current_tx_id: Option<TransactionId>,
    stats: BatchStats,
}

/// 트랜잭션 제공자 트레이트
pub trait TransactionProvider: Send + Sync {
    /// 새 트랜잭션 시작
    fn begin(&self) -> Result<TransactionId>;
    /// 트랜잭션 커밋
    fn commit(&self, tx_id: TransactionId) -> Result<()>;
    /// 트랜잭션 롤백
    fn rollback(&self, tx_id: TransactionId) -> Result<()>;
}

impl<T: TransactionProvider> BatchTransaction<T> {
    /// 새 배치 트랜잭션 생성
    pub fn new(provider: Arc<T>, config: BatchConfig) -> Self {
        Self {
            provider,
            config,
            pending_writes: Vec::new(),
            current_tx_id: None,
            stats: BatchStats::default(),
        }
    }

    /// 기본 설정으로 생성
    pub fn with_default(provider: Arc<T>) -> Self {
        Self::new(provider, BatchConfig::default())
    }

    /// 쓰기 작업 추가
    ///
    /// auto_flush가 활성화되고 배치 크기에 도달하면 자동으로 flush합니다.
    pub fn add<W: BatchWrite + 'static>(&mut self, write: W) -> Result<()> {
        self.pending_writes.push(Box::new(write));

        // 자동 flush 체크
        if self.config.auto_flush && self.pending_writes.len() >= self.config.batch_size {
            self.flush()?;
        }

        Ok(())
    }

    /// 현재까지의 작업을 flush (커밋)
    pub fn flush(&mut self) -> Result<()> {
        if self.pending_writes.is_empty() {
            return Ok(());
        }

        // 트랜잭션 시작
        let tx_id = self.provider.begin()?;
        self.current_tx_id = Some(tx_id);

        let mut success_count = 0;
        let mut fail_count = 0;
        let mut last_error: Option<GraphError> = None;

        // 작업 실행
        for write in self.pending_writes.drain(..) {
            match write.execute(tx_id) {
                Ok(()) => {
                    success_count += 1;
                }
                Err(e) => {
                    fail_count += 1;
                    tracing::warn!("Batch write failed: {} - {}", write.description(), e);

                    if self.config.rollback_on_error {
                        last_error = Some(e);
                        break;
                    }
                }
            }
        }

        // 통계 업데이트
        self.stats.total_operations += success_count + fail_count;
        self.stats.successful_operations += success_count;
        self.stats.failed_operations += fail_count;

        // 커밋 또는 롤백
        if let Some(e) = last_error {
            self.provider.rollback(tx_id)?;
            self.stats.rollbacks += 1;
            self.current_tx_id = None;
            return Err(e);
        }

        self.provider.commit(tx_id)?;
        self.stats.batch_commits += 1;
        self.current_tx_id = None;

        Ok(())
    }

    /// 모든 작업을 커밋하고 통계 반환
    pub fn commit(mut self) -> Result<BatchStats> {
        self.flush()?;
        let stats = self.stats.clone();
        self.pending_writes.clear(); // Drop 시 flush 시도 방지
        Ok(stats)
    }

    /// 롤백 (남은 작업 폐기)
    pub fn rollback(mut self) -> Result<BatchStats> {
        if let Some(tx_id) = self.current_tx_id.take() {
            self.provider.rollback(tx_id)?;
            self.stats.rollbacks += 1;
        }
        self.pending_writes.clear();
        let stats = self.stats.clone();
        Ok(stats)
    }

    /// 현재 통계 조회
    pub fn stats(&self) -> &BatchStats {
        &self.stats
    }

    /// 대기 중인 작업 수
    pub fn pending_count(&self) -> usize {
        self.pending_writes.len()
    }
}

impl<T: TransactionProvider> Drop for BatchTransaction<T> {
    fn drop(&mut self) {
        // 남은 작업이 있으면 flush 시도
        if !self.pending_writes.is_empty()
            && let Err(e) = self.flush()
        {
            tracing::error!("Failed to flush pending batch writes on drop: {}", e);
        }
    }
}

/// TransactionManager용 TransactionProvider 구현
impl TransactionProvider for TransactionManager {
    fn begin(&self) -> Result<TransactionId> {
        self.begin_transaction().map_err(Into::into)
    }

    fn commit(&self, tx_id: TransactionId) -> Result<()> {
        self.commit_transaction(tx_id).map_err(Into::into)
    }

    fn rollback(&self, tx_id: TransactionId) -> Result<()> {
        self.rollback_transaction(tx_id).map_err(Into::into)
    }
}
