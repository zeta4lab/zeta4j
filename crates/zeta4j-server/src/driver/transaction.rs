// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! M9.3: Transaction API
//!
//! 트랜잭션 관리

#![allow(dead_code)]

use indexmap::IndexMap;
use std::collections::HashMap;
use std::time::Duration;

use crate::server::bolt::BoltError;
use crate::server::bolt::packstream::PackStreamValue;

use super::error::{DriverError, DriverResult};
use super::pool::PooledConnection;
use super::record::Record;
use super::session::{Query, QueryResult, ResultSummary};
use super::types::Value;

/// BoltError를 DriverError로 변환
fn bolt_error_to_driver_error(err: BoltError) -> DriverError {
    match err {
        BoltError::Io(e) => DriverError::Connection(e.to_string()),
        BoltError::Protocol(msg) => DriverError::Protocol(msg),
        BoltError::Query(msg) => {
            // "code: message" 형태인 경우 파싱
            if let Some((code, message)) = msg.split_once(": ") {
                DriverError::query(code, message)
            } else {
                DriverError::query("Zeta.ClientError.Statement.ExecutionFailed", msg)
            }
        }
        BoltError::QueryExecution { code, message } => DriverError::query(code, message),
        BoltError::Handshake(e) => DriverError::Protocol(format!("Handshake: {}", e)),
        BoltError::PackStream(e) => DriverError::Protocol(format!("PackStream: {}", e)),
        BoltError::Connection(msg) => DriverError::Connection(msg),
        BoltError::Timeout => DriverError::timeout("Bolt operation timed out"),
        BoltError::Authentication(msg) => DriverError::Authentication(msg),
        BoltError::Transaction(msg) => DriverError::transaction(msg),
        BoltError::InvalidState(msg) => DriverError::transaction(format!("Invalid state: {}", msg)),
        BoltError::UnsupportedVersion(ver) => {
            DriverError::Protocol(format!("Unsupported version: {}", ver))
        }
        BoltError::MessageTooLarge { size, max } => {
            DriverError::Protocol(format!("Message too large: {} bytes (max: {})", size, max))
        }
        BoltError::ConnectionClosed => DriverError::Connection("Connection closed".to_string()),
        BoltError::ServerShutdown => DriverError::service_unavailable("Server shutdown"),
    }
}

// ============================================================================
// TransactionConfig - 트랜잭션 설정
// ============================================================================

/// 트랜잭션 설정
#[derive(Debug, Clone, Default)]
pub struct TransactionConfig {
    /// 타임아웃
    pub timeout: Option<Duration>,
    /// 메타데이터
    pub metadata: IndexMap<String, Value>,
}

impl TransactionConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 타임아웃 설정
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// 메타데이터 추가
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

// ============================================================================
// TransactionState - 트랜잭션 상태
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
    /// 실패
    Failed,
}

impl TransactionState {
    /// 완료 상태 여부
    pub fn is_terminated(&self) -> bool {
        !matches!(self, Self::Active)
    }
}

// ============================================================================
// Transaction - 트랜잭션
// ============================================================================

/// 명시적 트랜잭션
pub struct Transaction {
    /// 연결 (Option으로 래핑하여 Drop에서 이동 가능)
    connection: Option<PooledConnection>,
    /// 설정
    config: TransactionConfig,
    /// 데이터베이스
    database: Option<String>,
    /// 상태
    state: TransactionState,
}

impl Transaction {
    /// 트랜잭션 시작
    pub async fn begin(
        connection: PooledConnection,
        config: TransactionConfig,
        database: Option<String>,
    ) -> DriverResult<Self> {
        let mut tx = Self {
            connection: Some(connection),
            config,
            database,
            state: TransactionState::Active,
        };

        tx.send_begin().await?;
        Ok(tx)
    }

    /// 쿼리 실행
    pub async fn run(
        &mut self,
        query: impl Into<Query>,
        params: Option<IndexMap<String, Value>>,
    ) -> DriverResult<QueryResult> {
        self.ensure_active()?;

        let mut query = query.into();
        if let Some(p) = params {
            query = query.with_params(p);
        }

        self.execute_query(&query).await
    }

    /// 커밋
    pub async fn commit(mut self) -> DriverResult<()> {
        self.ensure_active()?;

        match self.send_commit().await {
            Ok(_) => {
                self.state = TransactionState::Committed;
                if let Some(conn) = self.connection.take() {
                    conn.return_to_pool();
                }
                Ok(())
            }
            Err(e) => {
                self.state = TransactionState::Failed;
                Err(e)
            }
        }
    }

    /// 롤백
    pub async fn rollback(mut self) -> DriverResult<()> {
        if self.state.is_terminated() {
            return Ok(());
        }

        match self.send_rollback().await {
            Ok(_) => {
                self.state = TransactionState::RolledBack;
                if let Some(conn) = self.connection.take() {
                    conn.return_to_pool();
                }
                Ok(())
            }
            Err(e) => {
                self.state = TransactionState::Failed;
                Err(e)
            }
        }
    }

    /// 트랜잭션 닫기
    pub async fn close(self) -> DriverResult<()> {
        if self.state.is_terminated() {
            return Ok(());
        }
        self.rollback().await
    }

    /// 활성 상태 확인
    fn ensure_active(&self) -> DriverResult<()> {
        match self.state {
            TransactionState::Active => Ok(()),
            TransactionState::Committed => {
                Err(DriverError::transaction("Transaction already committed"))
            }
            TransactionState::RolledBack => {
                Err(DriverError::transaction("Transaction already rolled back"))
            }
            TransactionState::Failed => {
                Err(DriverError::transaction("Transaction in failed state"))
            }
        }
    }

    /// BEGIN 전송
    async fn send_begin(&mut self) -> DriverResult<()> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| DriverError::transaction("No connection available"))?;

        let bolt_client = connection
            .bolt_client_mut()
            .ok_or_else(|| DriverError::transaction("No Bolt client available"))?;

        // 북마크 (추후 구현)
        let bookmarks = vec![];

        bolt_client
            .begin(self.database.as_deref(), bookmarks)
            .await
            .map_err(bolt_error_to_driver_error)
    }

    /// COMMIT 전송
    async fn send_commit(&mut self) -> DriverResult<()> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| DriverError::transaction("No connection available"))?;

        let bolt_client = connection
            .bolt_client_mut()
            .ok_or_else(|| DriverError::transaction("No Bolt client available"))?;

        bolt_client
            .commit()
            .await
            .map(|_| ()) // 북마크는 현재 무시
            .map_err(bolt_error_to_driver_error)
    }

    /// ROLLBACK 전송
    async fn send_rollback(&mut self) -> DriverResult<()> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| DriverError::transaction("No connection available"))?;

        let bolt_client = connection
            .bolt_client_mut()
            .ok_or_else(|| DriverError::transaction("No Bolt client available"))?;

        bolt_client
            .rollback()
            .await
            .map_err(bolt_error_to_driver_error)
    }

    /// 쿼리 실행 (내부)
    async fn execute_query(&mut self, query: &Query) -> DriverResult<QueryResult> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| DriverError::transaction("No connection available"))?;

        let bolt_client = connection
            .bolt_client_mut()
            .ok_or_else(|| DriverError::transaction("No Bolt client available"))?;

        // 파라미터 변환 (Value -> PackStreamValue)
        let parameters: HashMap<String, PackStreamValue> = query
            .parameters
            .iter()
            .map(|(k, v)| (k.clone(), v.clone().into()))
            .collect();

        // RUN + PULL 실행
        let bolt_result = bolt_client
            .run(&query.text, parameters, self.database.as_deref())
            .await
            .map_err(bolt_error_to_driver_error)?;

        // 결과 변환 (PackStreamValue -> Value)
        let keys = bolt_result.keys.clone();
        let records: Vec<Record> = bolt_result
            .records
            .into_iter()
            .map(|row| {
                let values: Vec<Value> = row.into_iter().map(|v| v.into()).collect();
                Record::new(keys.clone(), values)
            })
            .collect();

        // 통계 변환
        let summary = ResultSummary {
            query: Some(query.clone()),
            server: bolt_result.server,
            database: bolt_result.database,
            ..Default::default()
        };

        Ok(QueryResult::new(records, keys, summary))
    }

    /// 트랜잭션 상태
    pub fn state(&self) -> TransactionState {
        self.state
    }

    /// 데이터베이스
    pub fn database(&self) -> Option<&str> {
        self.database.as_deref()
    }

    /// 설정
    pub fn config(&self) -> &TransactionConfig {
        &self.config
    }
}

impl std::fmt::Debug for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("database", &self.database)
            .field("state", &self.state)
            .finish()
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        // 아직 활성 상태면 롤백 필요 (비동기라 여기서는 불가)
        if self.state == TransactionState::Active {
            // 경고: 트랜잭션이 명시적으로 커밋/롤백되지 않음
            // 연결은 풀로 반환되지만, 서버 측 트랜잭션은 타임아웃됨
            if let Some(conn) = self.connection.take() {
                conn.return_to_pool();
            }
        }
    }
}

// ============================================================================
// UnmanagedTransaction - 비관리 트랜잭션
// ============================================================================

/// 비관리 트랜잭션 (트랜잭션 함수용)
pub struct UnmanagedTransaction<'a> {
    /// 트랜잭션 참조
    tx: &'a mut Transaction,
}

impl<'a> UnmanagedTransaction<'a> {
    /// 새 비관리 트랜잭션 생성
    pub fn new(tx: &'a mut Transaction) -> Self {
        Self { tx }
    }

    /// 쿼리 실행
    pub async fn run(
        &mut self,
        query: impl Into<Query>,
        params: Option<IndexMap<String, Value>>,
    ) -> DriverResult<QueryResult> {
        self.tx.run(query, params).await
    }
}

// ============================================================================
// TransactionWork - 트랜잭션 작업
// ============================================================================

/// 트랜잭션 작업 트레이트
pub trait TransactionWork<T> {
    /// 작업 실행
    fn execute(&self, tx: Transaction)
    -> impl std::future::Future<Output = DriverResult<T>> + Send;
}
