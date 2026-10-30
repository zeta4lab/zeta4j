//! Transaction management for the Cypher shell.
//!
//! Bolt 프로토콜 기반 트랜잭션 관리자
//!
//! # 사용법
//!
//! ```text
//! :begin              # 트랜잭션 시작
//! CREATE (n:Test);    # 쿼리 실행 (트랜잭션 내)
//! :commit             # 커밋
//! :rollback           # 롤백
//! :tx-status          # 상태 확인
//! ```

use std::time::{Duration, Instant};

/// 트랜잭션 상태
#[derive(Debug, Clone)]
pub enum TransactionState {
    /// 트랜잭션 없음 (자동 커밋 모드)
    None,
    /// 활성 트랜잭션
    Active {
        /// 트랜잭션 ID (서버 할당)
        tx_id: String,
        /// 시작 시간
        started_at: Instant,
        /// 실행된 쿼리 수
        query_count: u32,
    },
    /// 실패한 트랜잭션 (롤백 필요)
    Failed {
        /// 실패 원인
        error: String,
        /// 시작 시간
        started_at: Instant,
    },
}

impl TransactionState {
    /// 활성 트랜잭션인지 확인
    pub fn is_active(&self) -> bool {
        matches!(self, TransactionState::Active { .. })
    }

    /// 실패 상태인지 확인
    pub fn is_failed(&self) -> bool {
        matches!(self, TransactionState::Failed { .. })
    }

    /// 트랜잭션 없는 상태인지 확인
    pub fn is_none(&self) -> bool {
        matches!(self, TransactionState::None)
    }

    /// 트랜잭션 ID 반환
    pub fn tx_id(&self) -> Option<&str> {
        match self {
            TransactionState::Active { tx_id, .. } => Some(tx_id),
            _ => None,
        }
    }

    /// 경과 시간 반환
    pub fn elapsed(&self) -> Option<Duration> {
        match self {
            TransactionState::Active { started_at, .. } => Some(started_at.elapsed()),
            TransactionState::Failed { started_at, .. } => Some(started_at.elapsed()),
            TransactionState::None => None,
        }
    }
}

/// 트랜잭션 관리자
#[derive(Debug)]
pub struct TransactionManager {
    /// 현재 상태
    state: TransactionState,
    /// 북마크 (Causal Consistency)
    bookmark: Option<String>,
    /// 트랜잭션 타임아웃
    timeout: Duration,
    /// 자동 롤백 활성화 (실패 시)
    auto_rollback: bool,
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self {
            state: TransactionState::None,
            bookmark: None,
            timeout: Duration::from_secs(30),
            auto_rollback: true,
        }
    }
}

impl TransactionManager {
    /// 새 트랜잭션 관리자 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 타임아웃 설정
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 자동 롤백 설정
    pub fn with_auto_rollback(mut self, enabled: bool) -> Self {
        self.auto_rollback = enabled;
        self
    }

    /// 현재 상태 반환
    pub fn state(&self) -> &TransactionState {
        &self.state
    }

    /// 북마크 반환
    pub fn bookmark(&self) -> Option<&str> {
        self.bookmark.as_deref()
    }

    /// 타임아웃 반환
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// 트랜잭션 시작
    ///
    /// # Returns
    /// - `Ok(())`: 성공
    /// - `Err(TransactionError)`: 이미 활성 트랜잭션이 있는 경우
    pub fn begin(&mut self, tx_id: String) -> Result<(), TransactionError> {
        match &self.state {
            TransactionState::None => {
                self.state = TransactionState::Active {
                    tx_id,
                    started_at: Instant::now(),
                    query_count: 0,
                };
                Ok(())
            }
            TransactionState::Active { tx_id, .. } => Err(TransactionError::AlreadyActive {
                tx_id: tx_id.clone(),
            }),
            TransactionState::Failed { error, .. } => Err(TransactionError::FailedTransaction {
                error: error.clone(),
            }),
        }
    }

    /// 쿼리 실행 카운트 증가
    pub fn increment_query_count(&mut self) {
        if let TransactionState::Active { query_count, .. } = &mut self.state {
            *query_count += 1;
        }
    }

    /// 트랜잭션 커밋
    ///
    /// # Arguments
    /// - `new_bookmark`: 서버에서 반환된 새 북마크
    pub fn commit(
        &mut self,
        new_bookmark: Option<String>,
    ) -> Result<TransactionCommitInfo, TransactionError> {
        match &self.state {
            TransactionState::Active {
                tx_id,
                started_at,
                query_count,
            } => {
                let info = TransactionCommitInfo {
                    tx_id: tx_id.clone(),
                    duration: started_at.elapsed(),
                    query_count: *query_count,
                    bookmark: new_bookmark.clone(),
                };

                self.bookmark = new_bookmark;
                self.state = TransactionState::None;

                Ok(info)
            }
            TransactionState::None => Err(TransactionError::NoActiveTransaction),
            TransactionState::Failed { error, .. } => Err(TransactionError::FailedTransaction {
                error: error.clone(),
            }),
        }
    }

    /// 트랜잭션 롤백
    pub fn rollback(&mut self) -> Result<TransactionRollbackInfo, TransactionError> {
        let info = match &self.state {
            TransactionState::Active {
                tx_id,
                started_at,
                query_count,
            } => TransactionRollbackInfo {
                tx_id: tx_id.clone(),
                duration: started_at.elapsed(),
                query_count: *query_count,
                was_failed: false,
            },
            TransactionState::Failed { started_at, .. } => TransactionRollbackInfo {
                tx_id: "unknown".to_string(),
                duration: started_at.elapsed(),
                query_count: 0,
                was_failed: true,
            },
            TransactionState::None => {
                return Err(TransactionError::NoActiveTransaction);
            }
        };

        self.state = TransactionState::None;
        Ok(info)
    }

    /// 트랜잭션 실패 표시
    pub fn mark_failed(&mut self, error: String) {
        if let TransactionState::Active { started_at, .. } = &self.state {
            self.state = TransactionState::Failed {
                error,
                started_at: *started_at,
            };
        }
    }

    /// 타임아웃 확인
    pub fn check_timeout(&self) -> Result<(), TransactionError> {
        if let Some(elapsed) = self.state.elapsed()
            && elapsed > self.timeout
        {
            return Err(TransactionError::Timeout {
                elapsed,
                timeout: self.timeout,
            });
        }
        Ok(())
    }

    /// 상태 문자열 반환 (CLI 출력용)
    pub fn status_string(&self) -> String {
        match &self.state {
            TransactionState::None => "No active transaction (auto-commit mode)".to_string(),
            TransactionState::Active {
                tx_id,
                started_at,
                query_count,
            } => {
                format!(
                    "Active transaction: {}\n  Elapsed: {:.2?}\n  Queries: {}",
                    tx_id,
                    started_at.elapsed(),
                    query_count
                )
            }
            TransactionState::Failed { error, started_at } => {
                format!(
                    "Failed transaction (rollback required)\n  Error: {}\n  Elapsed: {:.2?}",
                    error,
                    started_at.elapsed()
                )
            }
        }
    }

    /// 프롬프트 접미사 반환 (트랜잭션 상태 표시)
    pub fn prompt_suffix(&self) -> &'static str {
        match &self.state {
            TransactionState::None => "",
            TransactionState::Active { .. } => " [TX]",
            TransactionState::Failed { .. } => " [TX:FAILED]",
        }
    }
}

/// 트랜잭션 커밋 정보
#[derive(Debug, Clone)]
pub struct TransactionCommitInfo {
    /// 트랜잭션 ID
    pub tx_id: String,
    /// 소요 시간
    pub duration: Duration,
    /// 실행된 쿼리 수
    pub query_count: u32,
    /// 새 북마크
    pub bookmark: Option<String>,
}

/// 트랜잭션 롤백 정보
#[derive(Debug, Clone)]
pub struct TransactionRollbackInfo {
    /// 트랜잭션 ID
    pub tx_id: String,
    /// 소요 시간
    pub duration: Duration,
    /// 실행된 쿼리 수
    pub query_count: u32,
    /// 실패 상태였는지
    pub was_failed: bool,
}

/// 트랜잭션 에러
#[derive(Debug, Clone, thiserror::Error)]
pub enum TransactionError {
    /// 이미 활성 트랜잭션 존재
    #[error("Transaction already active: {tx_id}")]
    AlreadyActive { tx_id: String },

    /// 활성 트랜잭션 없음
    #[error("No active transaction")]
    NoActiveTransaction,

    /// 실패한 트랜잭션
    #[error("Transaction failed: {error}. Use :rollback to reset.")]
    FailedTransaction { error: String },

    /// 트랜잭션 타임아웃
    #[error("Transaction timeout: elapsed {elapsed:?}, timeout {timeout:?}")]
    Timeout {
        elapsed: Duration,
        timeout: Duration,
    },

    /// 서버 에러
    #[error("Server error: {0}")]
    ServerError(String),
}
