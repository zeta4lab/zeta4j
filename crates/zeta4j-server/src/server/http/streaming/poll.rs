//! Long Polling (M2.4)
//!
//! 대용량 쿼리 결과를 위한 Long Polling 지원

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;
use zeta4j_graph::storage::memory::MemoryReservation;

use super::types::{PollProgress, PollStatus};

/// Poll 에러
#[derive(Debug, Error)]
pub enum PollError {
    #[error("Query not found: {0}")]
    QueryNotFound(String),

    #[error("Query already completed")]
    QueryCompleted,

    #[error("Query failed: {0}")]
    QueryFailed(String),

    #[error("Query timeout")]
    QueryTimeout,

    #[error("Query cancelled")]
    QueryCancelled,

    #[error("Too many pending queries")]
    TooManyQueries,
}

/// 쿼리 상태
#[derive(Debug, Clone)]
pub struct QueryState {
    pub query_id: String,
    pub status: PollStatus,
    pub columns: Vec<String>,
    pub results: Vec<serde_json::Value>,
    pub cursor: usize,
    pub total_rows: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub error: Option<String>,
    pub memory_reservation: Option<Arc<MemoryReservation>>,
}

impl QueryState {
    /// 새 쿼리 상태 생성
    pub fn new(query_id: String) -> Self {
        let now = Utc::now();
        Self {
            query_id,
            status: PollStatus::Running,
            columns: Vec::new(),
            results: Vec::new(),
            cursor: 0,
            total_rows: None,
            created_at: now,
            updated_at: now,
            error: None,
            memory_reservation: None,
        }
    }

    /// 컬럼 정보 설정
    pub fn set_columns(&mut self, columns: Vec<String>) {
        self.columns = columns;
        self.updated_at = Utc::now();
    }

    /// 결과 추가
    pub fn add_results(&mut self, rows: Vec<serde_json::Value>) {
        self.results.extend(rows);
        self.updated_at = Utc::now();
    }

    /// 완료 표시
    pub fn complete(&mut self) {
        self.status = PollStatus::Completed;
        self.total_rows = Some(self.results.len() as u64);
        self.updated_at = Utc::now();
    }

    /// 실패 표시
    pub fn fail(&mut self, error: String) {
        self.status = PollStatus::Failed;
        self.error = Some(error);
        self.updated_at = Utc::now();
    }

    /// 취소 표시
    pub fn cancel(&mut self) {
        self.status = PollStatus::Cancelled;
        self.updated_at = Utc::now();
    }

    /// 다음 배치 가져오기
    pub fn fetch_batch(&mut self, batch_size: usize) -> Vec<serde_json::Value> {
        let start = self.cursor;
        let end = std::cmp::min(start + batch_size, self.results.len());
        self.cursor = end;
        self.updated_at = Utc::now();

        self.results[start..end].to_vec()
    }

    /// 진행 상황 가져오기
    pub fn get_progress(&self) -> PollProgress {
        let total = self.total_rows.or(Some(self.results.len() as u64));
        let percentage = total.map(|t| {
            if t == 0 {
                100.0
            } else {
                (self.cursor as f32 / t as f32) * 100.0
            }
        });

        PollProgress {
            rows_fetched: self.cursor as u64,
            total_rows: total,
            percentage,
        }
    }

    /// 남은 결과 여부
    pub fn has_more(&self) -> bool {
        self.cursor < self.results.len()
    }
}

/// Poll 관리자 설정
#[derive(Debug, Clone)]
pub struct PollManagerConfig {
    /// 최대 동시 쿼리 수
    pub max_queries: usize,
    /// 쿼리 만료 시간
    pub query_ttl: Duration,
    /// 정리 간격
    pub cleanup_interval: Duration,
    /// 기본 배치 크기
    pub default_batch_size: usize,
}

impl Default for PollManagerConfig {
    fn default() -> Self {
        Self {
            max_queries: 1000,
            query_ttl: Duration::from_secs(300),       // 5분
            cleanup_interval: Duration::from_secs(60), // 1분
            default_batch_size: 1000,
        }
    }
}

/// Poll 관리자
pub struct PollManager {
    queries: RwLock<HashMap<String, QueryState>>,
    config: PollManagerConfig,
}

impl PollManager {
    /// 새 Poll 관리자 생성
    pub fn new(config: PollManagerConfig) -> Self {
        Self {
            queries: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// 새 쿼리 등록
    pub async fn register_query(&self, query_id: String) -> Result<(), PollError> {
        let mut queries = self.queries.write().await;

        if queries.len() >= self.config.max_queries {
            return Err(PollError::TooManyQueries);
        }

        queries.insert(query_id.clone(), QueryState::new(query_id));
        Ok(())
    }

    /// 쿼리 상태 조회
    pub async fn get_query(&self, query_id: &str) -> Result<QueryState, PollError> {
        let queries = self.queries.read().await;
        queries
            .get(query_id)
            .cloned()
            .ok_or_else(|| PollError::QueryNotFound(query_id.to_string()))
    }

    /// 결과 추가
    pub async fn add_results(
        &self,
        query_id: &str,
        rows: Vec<serde_json::Value>,
    ) -> Result<(), PollError> {
        let mut queries = self.queries.write().await;
        let state = queries
            .get_mut(query_id)
            .ok_or_else(|| PollError::QueryNotFound(query_id.to_string()))?;

        if state.status != PollStatus::Running {
            return Err(PollError::QueryCompleted);
        }

        state.add_results(rows);
        Ok(())
    }

    /// Add result rows and their source-row reservation under one state lock.
    pub async fn add_results_with_memory(
        &self,
        query_id: &str,
        rows: Vec<serde_json::Value>,
        memory_reservation: Option<MemoryReservation>,
    ) -> Result<(), PollError> {
        let mut queries = self.queries.write().await;
        let state = queries
            .get_mut(query_id)
            .ok_or_else(|| PollError::QueryNotFound(query_id.to_string()))?;
        if state.status != PollStatus::Running {
            return Err(PollError::QueryCompleted);
        }
        state.add_results(rows);
        state.memory_reservation = memory_reservation.map(Arc::new);
        Ok(())
    }

    /// 쿼리 컬럼 정보 설정
    pub async fn set_columns(&self, query_id: &str, columns: Vec<String>) -> Result<(), PollError> {
        let mut queries = self.queries.write().await;
        let state = queries
            .get_mut(query_id)
            .ok_or_else(|| PollError::QueryNotFound(query_id.to_string()))?;
        state.set_columns(columns);
        Ok(())
    }

    /// 쿼리 완료 표시
    pub async fn complete_query(&self, query_id: &str) -> Result<(), PollError> {
        let mut queries = self.queries.write().await;
        let state = queries
            .get_mut(query_id)
            .ok_or_else(|| PollError::QueryNotFound(query_id.to_string()))?;

        state.complete();
        Ok(())
    }

    /// 쿼리 실패 표시
    pub async fn fail_query(&self, query_id: &str, error: String) -> Result<(), PollError> {
        let mut queries = self.queries.write().await;
        let state = queries
            .get_mut(query_id)
            .ok_or_else(|| PollError::QueryNotFound(query_id.to_string()))?;

        state.fail(error);
        Ok(())
    }

    /// 배치 가져오기
    pub async fn fetch_batch(
        &self,
        query_id: &str,
        batch_size: Option<usize>,
    ) -> Result<(Vec<serde_json::Value>, PollProgress, bool), PollError> {
        let batch_size = batch_size.unwrap_or(self.config.default_batch_size);

        let mut queries = self.queries.write().await;
        let state = queries
            .get_mut(query_id)
            .ok_or_else(|| PollError::QueryNotFound(query_id.to_string()))?;

        if state.status == PollStatus::Failed {
            return Err(PollError::QueryFailed(
                state.error.clone().unwrap_or_default(),
            ));
        }

        if state.status == PollStatus::Cancelled {
            return Err(PollError::QueryCancelled);
        }

        let results = state.fetch_batch(batch_size);
        let progress = state.get_progress();
        let has_more = state.has_more()
            || (state.status == PollStatus::Running && state.cursor >= state.results.len());

        Ok((results, progress, has_more))
    }

    /// 쿼리 취소
    pub async fn cancel_query(&self, query_id: &str) -> Result<(), PollError> {
        let mut queries = self.queries.write().await;
        let state = queries
            .get_mut(query_id)
            .ok_or_else(|| PollError::QueryNotFound(query_id.to_string()))?;

        state.cancel();
        Ok(())
    }

    /// 쿼리 삭제
    pub async fn remove_query(&self, query_id: &str) -> Result<(), PollError> {
        let mut queries = self.queries.write().await;
        queries
            .remove(query_id)
            .ok_or_else(|| PollError::QueryNotFound(query_id.to_string()))?;
        Ok(())
    }

    /// 만료된 쿼리 정리
    pub async fn cleanup_expired(&self) {
        let now = Utc::now();
        let ttl = chrono::Duration::from_std(self.config.query_ttl).unwrap_or_default();

        let mut queries = self.queries.write().await;
        queries.retain(|_, state| {
            let age = now - state.updated_at;
            age < ttl
        });
    }

    /// 현재 쿼리 수
    pub async fn query_count(&self) -> usize {
        self.queries.read().await.len()
    }

    /// 쿼리 ID 목록
    pub async fn query_ids(&self) -> Vec<String> {
        self.queries.read().await.keys().cloned().collect()
    }
}

impl Default for PollManager {
    fn default() -> Self {
        Self::new(PollManagerConfig::default())
    }
}

/// 공유 Poll 관리자
pub type SharedPollManager = Arc<PollManager>;

/// Poll 관리자 생성 헬퍼
pub fn create_poll_manager(config: PollManagerConfig) -> SharedPollManager {
    Arc::new(PollManager::new(config))
}
