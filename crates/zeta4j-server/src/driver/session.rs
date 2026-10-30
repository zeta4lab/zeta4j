//! M9.4: Session Management
//!
//! 세션 관리

use indexmap::IndexMap;

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use super::bolt::PackStreamValue;
use super::driver::DriverConfig;
use super::error::{DriverError, DriverResult};
use super::pool::{ConnectionPool, PooledConnection};
use super::record::{Record, RecordStream};
use super::transaction::{Transaction, TransactionConfig};
use super::types::Value;

// ============================================================================
// AccessMode - 접근 모드
// ============================================================================

/// 접근 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessMode {
    /// 읽기
    #[default]
    Read,
    /// 쓰기
    Write,
}

// ============================================================================
// Bookmark - 북마크
// ============================================================================

/// 인과적 일관성 북마크
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Bookmark {
    /// 북마크 값
    value: String,
}

impl Bookmark {
    /// 새 북마크 생성
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    /// 북마크 값
    pub fn value(&self) -> &str {
        &self.value
    }

    /// 빈 북마크 여부
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// 여러 북마크에서 생성
    pub fn from_bookmarks(bookmarks: &[Bookmark]) -> Self {
        if bookmarks.is_empty() {
            Self::new("")
        } else {
            // 가장 최신 북마크 사용
            bookmarks.last().cloned().unwrap_or_else(|| Self::new(""))
        }
    }
}

impl std::fmt::Display for Bookmark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl From<String> for Bookmark {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for Bookmark {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

// ============================================================================
// SessionConfig - 세션 설정
// ============================================================================

/// 세션 설정
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// 데이터베이스 이름
    pub database: Option<String>,
    /// Fetch Size
    pub fetch_size: usize,
    /// 기본 접근 모드
    pub default_access_mode: AccessMode,
    /// 북마크
    pub bookmarks: Vec<Bookmark>,
    /// 임퍼손트 사용자
    pub impersonated_user: Option<String>,
}

impl SessionConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 빌더 시작
    pub fn builder() -> SessionConfigBuilder {
        SessionConfigBuilder::new()
    }

    /// 데이터베이스 설정
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Fetch Size 설정
    pub fn with_fetch_size(mut self, size: usize) -> Self {
        self.fetch_size = size;
        self
    }

    /// 접근 모드 설정
    pub fn with_access_mode(mut self, mode: AccessMode) -> Self {
        self.default_access_mode = mode;
        self
    }

    /// 북마크 설정
    pub fn with_bookmarks(mut self, bookmarks: Vec<Bookmark>) -> Self {
        self.bookmarks = bookmarks;
        self
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            database: None,
            fetch_size: 1000,
            default_access_mode: AccessMode::Write,
            bookmarks: Vec::new(),
            impersonated_user: None,
        }
    }
}

// ============================================================================
// SessionConfigBuilder - 세션 설정 빌더
// ============================================================================

/// 세션 설정 빌더
#[derive(Debug, Default)]
pub struct SessionConfigBuilder {
    config: SessionConfig,
}

impl SessionConfigBuilder {
    /// 새 빌더 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 데이터베이스 설정
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.config.database = Some(database.into());
        self
    }

    /// Fetch Size 설정
    pub fn with_fetch_size(mut self, size: usize) -> Self {
        self.config.fetch_size = size;
        self
    }

    /// 읽기 모드로 설정
    pub fn with_read_access(mut self) -> Self {
        self.config.default_access_mode = AccessMode::Read;
        self
    }

    /// 쓰기 모드로 설정
    pub fn with_write_access(mut self) -> Self {
        self.config.default_access_mode = AccessMode::Write;
        self
    }

    /// 북마크 설정
    pub fn with_bookmarks(mut self, bookmarks: Vec<Bookmark>) -> Self {
        self.config.bookmarks = bookmarks;
        self
    }

    /// 북마크 추가
    pub fn with_bookmark(mut self, bookmark: Bookmark) -> Self {
        self.config.bookmarks.push(bookmark);
        self
    }

    /// 임퍼손트 사용자 설정
    pub fn with_impersonated_user(mut self, user: impl Into<String>) -> Self {
        self.config.impersonated_user = Some(user.into());
        self
    }

    /// 빌드
    pub fn build(self) -> SessionConfig {
        self.config
    }
}

// ============================================================================
// Query - 쿼리
// ============================================================================

/// 쿼리
#[derive(Debug, Clone)]
pub struct Query {
    /// 쿼리 텍스트
    pub text: String,
    /// 파라미터
    pub parameters: IndexMap<String, Value>,
}

impl Query {
    /// 새 쿼리 생성
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            parameters: IndexMap::new(),
        }
    }

    /// 파라미터 추가
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.parameters.insert(key.into(), value.into());
        self
    }

    /// 파라미터들 추가
    pub fn with_params(mut self, params: IndexMap<String, Value>) -> Self {
        self.parameters.extend(params);
        self
    }
}

impl From<&str> for Query {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Query {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

// ============================================================================
// ResultSummary - 결과 요약
// ============================================================================

/// 결과 요약
#[derive(Debug, Clone, Default)]
pub struct ResultSummary {
    /// 쿼리
    pub query: Option<Query>,
    /// 쿼리 타입
    pub query_type: QueryType,
    /// 카운터
    pub counters: Counters,
    /// 결과 대기 시간
    pub result_available_after: Duration,
    /// 결과 소비 시간
    pub result_consumed_after: Duration,
    /// 데이터베이스 정보
    pub database: Option<String>,
    /// 서버 정보
    pub server: Option<String>,
    /// 알림
    pub notifications: Vec<Notification>,
}

/// 쿼리 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryType {
    /// 읽기 전용
    #[default]
    ReadOnly,
    /// 읽기/쓰기
    ReadWrite,
    /// 쓰기 전용
    WriteOnly,
    /// 스키마 변경
    SchemaWrite,
}

/// 카운터
#[derive(Debug, Clone, Default)]
pub struct Counters {
    /// 생성된 노드 수
    pub nodes_created: i64,
    /// 삭제된 노드 수
    pub nodes_deleted: i64,
    /// 생성된 관계 수
    pub relationships_created: i64,
    /// 삭제된 관계 수
    pub relationships_deleted: i64,
    /// 설정된 속성 수
    pub properties_set: i64,
    /// 추가된 레이블 수
    pub labels_added: i64,
    /// 제거된 레이블 수
    pub labels_removed: i64,
    /// 생성된 인덱스 수
    pub indexes_added: i64,
    /// 제거된 인덱스 수
    pub indexes_removed: i64,
    /// 추가된 제약조건 수
    pub constraints_added: i64,
    /// 제거된 제약조건 수
    pub constraints_removed: i64,
}

impl Counters {
    /// 변경 사항 존재 여부
    pub fn contains_updates(&self) -> bool {
        self.nodes_created > 0
            || self.nodes_deleted > 0
            || self.relationships_created > 0
            || self.relationships_deleted > 0
            || self.properties_set > 0
            || self.labels_added > 0
            || self.labels_removed > 0
    }

    /// 스키마 변경 존재 여부
    pub fn contains_system_updates(&self) -> bool {
        self.indexes_added > 0
            || self.indexes_removed > 0
            || self.constraints_added > 0
            || self.constraints_removed > 0
    }
}

/// 알림
#[derive(Debug, Clone)]
pub struct Notification {
    /// 코드
    pub code: String,
    /// 제목
    pub title: String,
    /// 설명
    pub description: String,
    /// 심각도
    pub severity: String,
    /// 위치
    pub position: Option<InputPosition>,
}

/// 입력 위치
#[derive(Debug, Clone)]
pub struct InputPosition {
    /// 오프셋
    pub offset: i64,
    /// 라인
    pub line: i64,
    /// 컬럼
    pub column: i64,
}

// ============================================================================
// Result - 쿼리 결과
// ============================================================================

/// 쿼리 결과
#[derive(Debug)]
pub struct QueryResult {
    /// 레코드 스트림
    pub records: RecordStream,
    /// 컬럼 키
    pub keys: Vec<String>,
    /// 결과 요약
    pub summary: ResultSummary,
}

impl QueryResult {
    /// 새 결과 생성
    pub fn new(records: Vec<Record>, keys: Vec<String>, summary: ResultSummary) -> Self {
        Self {
            records: RecordStream::new(records),
            keys,
            summary,
        }
    }

    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            records: RecordStream::empty(),
            keys: Vec::new(),
            summary: ResultSummary::default(),
        }
    }

    /// 단일 레코드 가져오기
    pub fn single(self) -> DriverResult<Record> {
        self.records.single()
    }

    /// 첫 번째 레코드 가져오기
    pub fn first(self) -> Option<Record> {
        self.records.first()
    }

    /// 모든 레코드 가져오기
    pub fn collect(self) -> Vec<Record> {
        self.records.collect_all()
    }
}

impl Iterator for QueryResult {
    type Item = Record;

    fn next(&mut self) -> Option<Self::Item> {
        self.records.next()
    }
}

// ============================================================================
// Session - 세션
// ============================================================================

/// 데이터베이스 세션
pub struct Session {
    /// 드라이버 설정
    driver_config: Arc<DriverConfig>,
    /// 연결 풀
    pool: Arc<ConnectionPool>,
    /// 세션 설정
    config: SessionConfig,
    /// 현재 북마크
    last_bookmark: RwLock<Option<Bookmark>>,
    /// 열린 상태
    open: RwLock<bool>,
}

impl Session {
    /// 새 세션 생성
    pub fn new(
        driver_config: Arc<DriverConfig>,
        pool: Arc<ConnectionPool>,
        config: SessionConfig,
    ) -> DriverResult<Self> {
        Ok(Self {
            driver_config,
            pool,
            config,
            last_bookmark: RwLock::new(None),
            open: RwLock::new(true),
        })
    }

    /// 쿼리 실행 (auto-commit)
    pub async fn run(
        &self,
        query: impl Into<Query>,
        params: Option<IndexMap<String, Value>>,
    ) -> DriverResult<QueryResult> {
        self.ensure_open()?;

        let mut query = query.into();
        if let Some(p) = params {
            query = query.with_params(p);
        }

        let mut conn = self.pool.acquire().await?;
        let result = self.execute_query(&mut conn, &query).await?;
        conn.return_to_pool();

        Ok(result)
    }

    /// 트랜잭션 시작
    pub async fn begin_transaction(
        &self,
        config: Option<TransactionConfig>,
    ) -> DriverResult<Transaction> {
        self.ensure_open()?;

        let conn = self.pool.acquire().await?;
        let config = config.unwrap_or_default();

        Transaction::begin(conn, config, self.config.database.clone()).await
    }

    /// 읽기 트랜잭션 함수
    pub async fn read_transaction<F, Fut, T>(&self, work: F) -> DriverResult<T>
    where
        F: Fn(Transaction) -> Fut,
        Fut: Future<Output = DriverResult<T>>,
    {
        self.execute_transaction(AccessMode::Read, work).await
    }

    /// 쓰기 트랜잭션 함수
    pub async fn write_transaction<F, Fut, T>(&self, work: F) -> DriverResult<T>
    where
        F: Fn(Transaction) -> Fut,
        Fut: Future<Output = DriverResult<T>>,
    {
        self.execute_transaction(AccessMode::Write, work).await
    }

    /// 트랜잭션 함수 실행 (재시도 포함)
    async fn execute_transaction<F, Fut, T>(&self, _mode: AccessMode, work: F) -> DriverResult<T>
    where
        F: Fn(Transaction) -> Fut,
        Fut: Future<Output = DriverResult<T>>,
    {
        self.ensure_open()?;

        let max_retry_time = self.driver_config.max_transaction_retry_time;
        let start = std::time::Instant::now();
        let mut attempts = 0;

        loop {
            attempts += 1;

            let tx = self.begin_transaction(None).await?;

            match work(tx).await {
                Ok(result) => return Ok(result),
                Err(e) if e.is_retryable() && start.elapsed() < max_retry_time => {
                    // 재시도 대기
                    let delay = std::cmp::min(
                        Duration::from_millis(100 * attempts),
                        Duration::from_secs(5),
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// 쿼리 실행 (내부)
    async fn execute_query(
        &self,
        conn: &mut PooledConnection,
        query: &Query,
    ) -> DriverResult<QueryResult> {
        // BoltClient로 실제 쿼리 실행
        let client = conn
            .bolt_client_mut()
            .ok_or_else(|| DriverError::connection("No Bolt connection available"))?;

        // 파라미터 변환
        let parameters: HashMap<String, PackStreamValue> = query
            .parameters
            .iter()
            .map(|(k, v)| (k.clone(), v.clone().into()))
            .collect();

        // 쿼리 실행
        let bolt_result = client
            .run(&query.text, parameters, self.config.database.as_deref())
            .await
            .map_err(|e| DriverError::query("QueryExecutionError", format!("{}", e)))?;

        // 북마크 업데이트
        if let Some(bookmark_str) = &bolt_result.bookmark {
            let bookmark = Bookmark::new(bookmark_str);
            *self.last_bookmark.write() = Some(bookmark);
        }

        // keys 복사 (move 방지)
        let keys = bolt_result.keys.clone();

        // Record 변환
        let records: Vec<Record> = bolt_result
            .records
            .into_iter()
            .map(|fields| {
                let values: Vec<Value> = fields.into_iter().map(Into::into).collect();
                Record::new(keys.clone(), values)
            })
            .collect();

        // Summary 생성
        let counters = bolt_result
            .stats
            .map(|stats| Counters {
                nodes_created: stats
                    .get("nodes-created")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                nodes_deleted: stats
                    .get("nodes-deleted")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                relationships_created: stats
                    .get("relationships-created")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                relationships_deleted: stats
                    .get("relationships-deleted")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                properties_set: stats
                    .get("properties-set")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                labels_added: stats
                    .get("labels-added")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                labels_removed: stats
                    .get("labels-removed")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                indexes_added: stats
                    .get("indexes-added")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                indexes_removed: stats
                    .get("indexes-removed")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                constraints_added: stats
                    .get("constraints-added")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
                constraints_removed: stats
                    .get("constraints-removed")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0),
            })
            .unwrap_or_default();

        let summary = ResultSummary {
            query: Some(query.clone()),
            counters,
            database: bolt_result.database,
            ..Default::default()
        };

        Ok(QueryResult::new(records, keys, summary))
    }

    /// 마지막 북마크
    pub fn last_bookmark(&self) -> Option<Bookmark> {
        self.last_bookmark.read().clone()
    }

    /// 모든 북마크
    pub fn last_bookmarks(&self) -> Vec<Bookmark> {
        let mut bookmarks = self.config.bookmarks.clone();
        if let Some(bookmark) = self.last_bookmark() {
            bookmarks.push(bookmark);
        }
        bookmarks
    }

    /// 세션 닫기
    pub async fn close(&self) -> DriverResult<()> {
        *self.open.write() = false;
        Ok(())
    }

    /// 열린 상태 확인
    fn ensure_open(&self) -> DriverResult<()> {
        if *self.open.read() {
            Ok(())
        } else {
            Err(DriverError::session("Session is closed"))
        }
    }

    /// 세션 설정
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("database", &self.config.database)
            .field("open", &*self.open.read())
            .finish()
    }
}
