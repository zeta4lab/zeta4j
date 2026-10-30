//! HTTP Transaction Manager
//!
//! HTTP API를 통한 트랜잭션 관리

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use uuid::Uuid;

use super::cypher_types::QueryStats;
use super::error::HttpError;

// ============================================================================
// Configuration
// ============================================================================

/// HTTP 트랜잭션 설정
#[derive(Debug, Clone)]
pub struct HttpTransactionConfig {
    /// 트랜잭션 타임아웃 (기본: 30초)
    pub timeout: Duration,

    /// 최대 동시 트랜잭션 수 (기본: 1000)
    pub max_transactions: usize,

    /// 정리 주기 (기본: 10초)
    pub cleanup_interval: Duration,
}

impl Default for HttpTransactionConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_transactions: 1000,
            cleanup_interval: Duration::from_secs(10),
        }
    }
}

impl HttpTransactionConfig {
    /// 빌더 생성
    pub fn builder() -> HttpTransactionConfigBuilder {
        HttpTransactionConfigBuilder::default()
    }
}

/// 설정 빌더
#[derive(Default)]
pub struct HttpTransactionConfigBuilder {
    config: HttpTransactionConfig,
}

impl HttpTransactionConfigBuilder {
    /// 타임아웃 설정
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// 타임아웃 (초) 설정
    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.config.timeout = Duration::from_secs(secs);
        self
    }

    /// 최대 트랜잭션 수 설정
    pub fn max_transactions(mut self, max: usize) -> Self {
        self.config.max_transactions = max;
        self
    }

    /// 정리 주기 설정
    pub fn cleanup_interval(mut self, interval: Duration) -> Self {
        self.config.cleanup_interval = interval;
        self
    }

    /// 설정 빌드
    pub fn build(self) -> HttpTransactionConfig {
        self.config
    }
}

// ============================================================================
// Transaction State
// ============================================================================

/// 트랜잭션 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    /// 활성 상태
    Active,
    /// 커밋됨
    Committed,
    /// 롤백됨
    RolledBack,
    /// 만료됨
    Expired,
}

/// Canonical owner bound to a transaction at creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionOwner {
    pub principal_id: String,
    pub security_generation: u64,
    pub tenant_id: String,
    pub database_id: String,
}

/// HTTP 트랜잭션
#[derive(Debug)]
pub struct HttpTransaction {
    /// 트랜잭션 ID
    pub id: String,

    /// 데이터베이스 이름
    pub db_name: String,

    /// 상태
    pub state: TransactionState,

    /// 생성 시간
    pub created_at: Instant,

    /// 만료 시간
    pub expires_at: Instant,

    /// 마지막 접근 시간
    pub last_access: Instant,

    /// 만료 시간 (DateTime)
    pub expires_datetime: DateTime<Utc>,

    /// 누적 통계
    pub stats: QueryStats,

    /// 실행된 쿼리 수
    pub query_count: usize,

    /// Principal and canonical scope that alone may use this transaction.
    pub owner: TransactionOwner,
}

impl HttpTransaction {
    /// 새 트랜잭션 생성
    pub fn new(db_name: String, owner: TransactionOwner, timeout: Duration) -> Self {
        let now = Instant::now();
        let expires_at = now + timeout;
        let expires_datetime = Utc::now() + chrono::Duration::from_std(timeout).unwrap_or_default();

        Self {
            id: Uuid::now_v7().to_string(),
            db_name,
            state: TransactionState::Active,
            created_at: now,
            expires_at,
            last_access: now,
            expires_datetime,
            stats: QueryStats::default(),
            query_count: 0,
            owner,
        }
    }

    /// 만료 여부 확인
    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at || self.state == TransactionState::Expired
    }

    /// 활성 여부 확인
    pub fn is_active(&self) -> bool {
        self.state == TransactionState::Active && !self.is_expired()
    }

    /// 만료 시간 갱신 (접근 시 타임아웃 연장)
    pub fn touch(&mut self, timeout: Duration) {
        self.last_access = Instant::now();
        self.expires_at = self.last_access + timeout;
        self.expires_datetime =
            Utc::now() + chrono::Duration::from_std(timeout).unwrap_or_default();
    }

    /// 만료 시간 문자열 (ISO 8601)
    pub fn expires_string(&self) -> String {
        self.expires_datetime.to_rfc3339()
    }

    /// 통계 병합
    pub fn merge_stats(&mut self, other: &QueryStats) {
        self.stats.nodes_created += other.nodes_created;
        self.stats.nodes_deleted += other.nodes_deleted;
        self.stats.properties_set += other.properties_set;
        self.stats.relationships_created += other.relationships_created;
        self.stats.relationships_deleted += other.relationships_deleted;
        self.stats.labels_added += other.labels_added;
        self.stats.labels_removed += other.labels_removed;
        self.stats.indexes_added += other.indexes_added;
        self.stats.indexes_removed += other.indexes_removed;
        self.stats.constraints_added += other.constraints_added;
        self.stats.constraints_removed += other.constraints_removed;
        self.stats.compute_contains_updates();
    }
}

// ============================================================================
// Transaction Manager
// ============================================================================

/// HTTP 트랜잭션 매니저
pub struct HttpTransactionManager {
    /// 활성 트랜잭션 맵
    transactions: RwLock<HashMap<String, HttpTransaction>>,

    /// 설정
    config: HttpTransactionConfig,
}

impl HttpTransactionManager {
    /// 새 매니저 생성
    pub fn new(config: HttpTransactionConfig) -> Self {
        Self {
            transactions: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// 기본 설정으로 생성
    pub fn default_manager() -> Self {
        Self::new(HttpTransactionConfig::default())
    }

    /// 새 트랜잭션 시작
    pub fn begin(&self, db_name: &str, owner: TransactionOwner) -> Result<String, HttpError> {
        let mut transactions = self.transactions.write();

        // 최대 트랜잭션 수 확인
        if transactions.len() >= self.config.max_transactions {
            // 만료된 트랜잭션 정리 시도
            transactions.retain(|_, tx| tx.is_active());

            if transactions.len() >= self.config.max_transactions {
                return Err(HttpError::TooManyRequests);
            }
        }

        let tx = HttpTransaction::new(db_name.to_string(), owner, self.config.timeout);
        let tx_id = tx.id.clone();
        transactions.insert(tx_id.clone(), tx);

        Ok(tx_id)
    }

    /// 트랜잭션 조회
    pub fn get(&self, tx_id: &str) -> Result<HttpTransaction, HttpError> {
        let transactions = self.transactions.read();

        match transactions.get(tx_id) {
            Some(tx) => {
                if tx.is_expired() {
                    Err(HttpError::BadRequest(format!(
                        "Transaction {} has expired",
                        tx_id
                    )))
                } else if tx.state != TransactionState::Active {
                    Err(HttpError::BadRequest(format!(
                        "Transaction {} is not active",
                        tx_id
                    )))
                } else {
                    // Clone the transaction data we need
                    Ok(HttpTransaction {
                        id: tx.id.clone(),
                        db_name: tx.db_name.clone(),
                        state: tx.state,
                        created_at: tx.created_at,
                        expires_at: tx.expires_at,
                        last_access: tx.last_access,
                        expires_datetime: tx.expires_datetime,
                        stats: tx.stats.clone(),
                        query_count: tx.query_count,
                        owner: tx.owner.clone(),
                    })
                }
            }
            None => Err(HttpError::NotFound(format!(
                "Transaction {} not found",
                tx_id
            ))),
        }
    }

    /// Verifies transaction ownership without disclosing whether another owner has it.
    pub fn verify_owner(
        &self,
        tx_id: &str,
        db_name: &str,
        owner: &TransactionOwner,
    ) -> Result<(), HttpError> {
        let transactions = self.transactions.read();
        let transaction = transactions
            .get(tx_id)
            .ok_or_else(|| HttpError::NotFound("Transaction not found".to_string()))?;
        if transaction.db_name != db_name || &transaction.owner != owner {
            return Err(HttpError::NotFound("Transaction not found".to_string()));
        }
        if !transaction.is_active() {
            return Err(HttpError::BadRequest(
                "Transaction is not active".to_string(),
            ));
        }
        Ok(())
    }

    /// 트랜잭션 타임아웃 갱신
    pub fn touch(&self, tx_id: &str) -> Result<String, HttpError> {
        let mut transactions = self.transactions.write();

        match transactions.get_mut(tx_id) {
            Some(tx) => {
                if tx.is_expired() {
                    tx.state = TransactionState::Expired;
                    Err(HttpError::BadRequest(format!(
                        "Transaction {} has expired",
                        tx_id
                    )))
                } else if tx.state != TransactionState::Active {
                    Err(HttpError::BadRequest(format!(
                        "Transaction {} is not active",
                        tx_id
                    )))
                } else {
                    tx.touch(self.config.timeout);
                    Ok(tx.expires_string())
                }
            }
            None => Err(HttpError::NotFound(format!(
                "Transaction {} not found",
                tx_id
            ))),
        }
    }

    /// 쿼리 실행 기록
    pub fn record_execution(&self, tx_id: &str, stats: &QueryStats) -> Result<(), HttpError> {
        let mut transactions = self.transactions.write();

        match transactions.get_mut(tx_id) {
            Some(tx) => {
                if !tx.is_active() {
                    return Err(HttpError::BadRequest(format!(
                        "Transaction {} is not active",
                        tx_id
                    )));
                }
                tx.touch(self.config.timeout);
                tx.merge_stats(stats);
                tx.query_count += 1;
                Ok(())
            }
            None => Err(HttpError::NotFound(format!(
                "Transaction {} not found",
                tx_id
            ))),
        }
    }

    /// 트랜잭션 커밋
    pub fn commit(&self, tx_id: &str) -> Result<QueryStats, HttpError> {
        let mut transactions = self.transactions.write();

        match transactions.get_mut(tx_id) {
            Some(tx) => {
                if tx.is_expired() {
                    tx.state = TransactionState::Expired;
                    return Err(HttpError::BadRequest(format!(
                        "Transaction {} has expired",
                        tx_id
                    )));
                }
                if tx.state != TransactionState::Active {
                    return Err(HttpError::BadRequest(format!(
                        "Transaction {} is not active",
                        tx_id
                    )));
                }
                tx.state = TransactionState::Committed;
                let stats = tx.stats.clone();

                // 커밋된 트랜잭션 제거
                transactions.remove(tx_id);

                Ok(stats)
            }
            None => Err(HttpError::NotFound(format!(
                "Transaction {} not found",
                tx_id
            ))),
        }
    }

    /// 트랜잭션 롤백
    pub fn rollback(&self, tx_id: &str) -> Result<(), HttpError> {
        let mut transactions = self.transactions.write();

        match transactions.get_mut(tx_id) {
            Some(tx) => {
                tx.state = TransactionState::RolledBack;
                transactions.remove(tx_id);
                Ok(())
            }
            None => Err(HttpError::NotFound(format!(
                "Transaction {} not found",
                tx_id
            ))),
        }
    }

    /// 강제 트랜잭션 롤백 (세션 타임아웃용)
    ///
    /// `rollback`과 달리 트랜잭션이 없거나 이미 종료되어도 에러를 반환하지 않음.
    /// 세션 만료 시 활성 트랜잭션을 자동 롤백할 때 사용.
    pub fn force_rollback(&self, tx_id: &str) -> bool {
        let mut transactions = self.transactions.write();

        if let Some(tx) = transactions.get_mut(tx_id)
            && tx.state == TransactionState::Active
        {
            tx.state = TransactionState::RolledBack;
            transactions.remove(tx_id);
            tracing::info!("Force rolled back transaction {}", tx_id);
            return true;
        }

        false
    }

    /// 만료된 트랜잭션 정리 및 롤백
    ///
    /// 만료된 트랜잭션을 찾아 롤백하고 제거.
    /// 롤백된 트랜잭션 ID 목록 반환.
    pub fn cleanup_and_rollback_expired(&self) -> Vec<String> {
        let mut transactions = self.transactions.write();
        let mut rolled_back = Vec::new();

        // 만료된 트랜잭션 ID 수집
        let expired_ids: Vec<String> = transactions
            .iter()
            .filter(|(_, tx)| tx.is_expired() && tx.state == TransactionState::Active)
            .map(|(id, _)| id.clone())
            .collect();

        // 롤백 및 제거
        for tx_id in expired_ids {
            if let Some(tx) = transactions.get_mut(&tx_id) {
                tx.state = TransactionState::RolledBack;
                tracing::info!("Auto-rolled back expired transaction {}", tx_id);
                rolled_back.push(tx_id.clone());
            }
            transactions.remove(&tx_id);
        }

        rolled_back
    }

    /// 만료된 트랜잭션 정리
    pub fn cleanup_expired(&self) -> usize {
        let mut transactions = self.transactions.write();
        let before = transactions.len();
        transactions.retain(|_, tx| tx.is_active());
        before - transactions.len()
    }

    /// 활성 트랜잭션 수
    pub fn active_count(&self) -> usize {
        let transactions = self.transactions.read();
        transactions.values().filter(|tx| tx.is_active()).count()
    }

    /// 전체 트랜잭션 수
    pub fn total_count(&self) -> usize {
        self.transactions.read().len()
    }

    /// 특정 DB의 트랜잭션 수
    pub fn count_for_db(&self, db_name: &str) -> usize {
        let transactions = self.transactions.read();
        transactions
            .values()
            .filter(|tx| tx.db_name == db_name && tx.is_active())
            .count()
    }
}

impl Default for HttpTransactionManager {
    fn default() -> Self {
        Self::default_manager()
    }
}
