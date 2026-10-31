// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server Session Management
//!
//! Phase 2 Step 5 M5.2: 세션 상태 관리
//!
//! 기능:
//! - 서버 세션 상태 관리
//! - 세션 메타데이터
//! - 세션 설정

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::bookmark::BookmarkManager;
use super::client::ClientInfo;

/// 서버 세션 ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerSessionId(u64);

impl ServerSessionId {
    /// 새 세션 ID 생성
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// 특정 값으로 생성
    pub fn from_value(value: u64) -> Self {
        Self(value)
    }

    /// ID 값 조회
    pub fn value(&self) -> u64 {
        self.0
    }

    /// 연결 ID 문자열
    pub fn as_connection_id(&self) -> String {
        format!("session-{}", self.0)
    }
}

impl Default for ServerSessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ServerSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session-{}", self.0)
    }
}

/// 세션 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// 초기화됨
    Initialized,
    /// 인증됨
    Authenticated,
    /// 활성 (쿼리 실행 중)
    Active,
    /// 트랜잭션 진행 중
    InTransaction,
    /// 유휴
    Idle,
    /// 종료됨
    Terminated,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionState::Initialized => write!(f, "INITIALIZED"),
            SessionState::Authenticated => write!(f, "AUTHENTICATED"),
            SessionState::Active => write!(f, "ACTIVE"),
            SessionState::InTransaction => write!(f, "IN_TRANSACTION"),
            SessionState::Idle => write!(f, "IDLE"),
            SessionState::Terminated => write!(f, "TERMINATED"),
        }
    }
}

/// 세션 설정
#[derive(Debug, Clone)]
pub struct SessionSettings {
    /// 세션 타임아웃 (유휴 시)
    pub session_timeout: Duration,
    /// 기본 트랜잭션 타임아웃
    pub default_tx_timeout: Duration,
    /// 기본 쿼리 타임아웃
    pub default_query_timeout: Duration,
    /// 기본 페치 크기
    pub fetch_size: usize,
    /// 최대 쿼리 결과 수
    pub max_records: Option<usize>,
    /// 읽기 전용 모드
    pub read_only: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            session_timeout: Duration::from_secs(3600), // 1시간
            default_tx_timeout: Duration::from_secs(30),
            default_query_timeout: Duration::from_secs(30),
            fetch_size: 1000,
            max_records: None,
            read_only: false,
        }
    }
}

impl SessionSettings {
    /// 빌더 생성
    pub fn builder() -> SessionSettingsBuilder {
        SessionSettingsBuilder::default()
    }
}

/// 세션 설정 빌더
#[derive(Default)]
pub struct SessionSettingsBuilder {
    session_timeout: Option<Duration>,
    default_tx_timeout: Option<Duration>,
    default_query_timeout: Option<Duration>,
    fetch_size: Option<usize>,
    max_records: Option<usize>,
    read_only: Option<bool>,
}

impl SessionSettingsBuilder {
    pub fn session_timeout(mut self, timeout: Duration) -> Self {
        self.session_timeout = Some(timeout);
        self
    }

    pub fn default_tx_timeout(mut self, timeout: Duration) -> Self {
        self.default_tx_timeout = Some(timeout);
        self
    }

    pub fn default_query_timeout(mut self, timeout: Duration) -> Self {
        self.default_query_timeout = Some(timeout);
        self
    }

    pub fn fetch_size(mut self, size: usize) -> Self {
        self.fetch_size = Some(size);
        self
    }

    pub fn max_records(mut self, max: usize) -> Self {
        self.max_records = Some(max);
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = Some(read_only);
        self
    }

    pub fn build(self) -> SessionSettings {
        let default = SessionSettings::default();
        SessionSettings {
            session_timeout: self.session_timeout.unwrap_or(default.session_timeout),
            default_tx_timeout: self
                .default_tx_timeout
                .unwrap_or(default.default_tx_timeout),
            default_query_timeout: self
                .default_query_timeout
                .unwrap_or(default.default_query_timeout),
            fetch_size: self.fetch_size.unwrap_or(default.fetch_size),
            max_records: self.max_records.or(default.max_records),
            read_only: self.read_only.unwrap_or(default.read_only),
        }
    }
}

/// 인증 상태
#[derive(Debug, Clone, Default)]
pub struct AuthState {
    /// 인증된 사용자
    pub principal: Option<String>,
    /// 인증 시간
    pub authenticated_at: Option<Instant>,
    /// 인증 방법
    pub auth_method: Option<String>,
}

impl AuthState {
    /// 인증 여부
    pub fn is_authenticated(&self) -> bool {
        self.principal.is_some()
    }

    /// 인증 설정
    pub fn authenticate(&mut self, principal: &str, method: &str) {
        self.principal = Some(principal.to_string());
        self.authenticated_at = Some(Instant::now());
        self.auth_method = Some(method.to_string());
    }

    /// 인증 해제
    pub fn logout(&mut self) {
        self.principal = None;
        self.authenticated_at = None;
        self.auth_method = None;
    }
}

/// 확장 트랜잭션 컨텍스트
#[derive(Debug, Clone)]
pub struct ExtendedTransactionContext {
    /// 트랜잭션 ID
    pub tx_id: u64,
    /// 읽기/쓰기 모드
    pub mode: TransactionMode,
    /// 대상 데이터베이스
    pub database: String,
    /// 타임아웃
    pub timeout: Duration,
    /// 대기 북마크
    pub bookmarks: Vec<String>,
    /// 메타데이터
    pub metadata: HashMap<String, String>,
    /// 시작 시간
    pub started_at: Instant,
    /// 쿼리 카운터
    pub query_count: u32,
    /// 수정된 노드 수
    pub nodes_modified: u64,
    /// 수정된 관계 수
    pub relationships_modified: u64,
}

impl ExtendedTransactionContext {
    /// 새 트랜잭션 컨텍스트 생성
    pub fn new(tx_id: u64, database: &str, mode: TransactionMode, timeout: Duration) -> Self {
        Self {
            tx_id,
            mode,
            database: database.to_string(),
            timeout,
            bookmarks: Vec::new(),
            metadata: HashMap::new(),
            started_at: Instant::now(),
            query_count: 0,
            nodes_modified: 0,
            relationships_modified: 0,
        }
    }

    /// 타임아웃 체크
    pub fn is_timed_out(&self) -> bool {
        self.started_at.elapsed() > self.timeout
    }

    /// 경과 시간
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// 쿼리 카운트 증가
    pub fn increment_query_count(&mut self) {
        self.query_count += 1;
    }
}

/// 트랜잭션 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransactionMode {
    /// 읽기 전용
    Read,
    /// 읽기/쓰기
    #[default]
    Write,
}

/// 세션 메타데이터 값
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetadataValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<MetadataValue>),
    Map(HashMap<String, MetadataValue>),
    Null,
}

impl MetadataValue {
    /// 문자열로 변환
    pub fn as_str(&self) -> Option<&str> {
        match self {
            MetadataValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// 정수로 변환
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            MetadataValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// 부울로 변환
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            MetadataValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }
}

impl From<String> for MetadataValue {
    fn from(s: String) -> Self {
        MetadataValue::String(s)
    }
}

impl From<&str> for MetadataValue {
    fn from(s: &str) -> Self {
        MetadataValue::String(s.to_string())
    }
}

impl From<i64> for MetadataValue {
    fn from(i: i64) -> Self {
        MetadataValue::Integer(i)
    }
}

impl From<bool> for MetadataValue {
    fn from(b: bool) -> Self {
        MetadataValue::Boolean(b)
    }
}

/// 서버 세션
///
/// 클라이언트 연결의 전체 상태를 관리하는 세션
#[derive(Debug)]
pub struct ServerSession {
    /// 세션 ID
    id: ServerSessionId,
    /// 클라이언트 정보
    client: Option<ClientInfo>,
    /// 인증 상태
    auth_state: AuthState,
    /// 세션 상태
    state: SessionState,
    /// 현재 데이터베이스
    database: String,
    /// 트랜잭션 컨텍스트
    transaction: Option<ExtendedTransactionContext>,
    /// 북마크 관리자
    bookmarks: BookmarkManager,
    /// 세션 설정
    settings: SessionSettings,
    /// 생성 시간
    created_at: Instant,
    /// 마지막 활동 시간
    last_activity: Instant,
    /// 세션 메타데이터
    metadata: HashMap<String, MetadataValue>,
    /// 총 쿼리 수
    total_queries: u64,
    /// 총 트랜잭션 수
    total_transactions: u64,
}

impl ServerSession {
    /// 새 세션 생성
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            id: ServerSessionId::new(),
            client: None,
            auth_state: AuthState::default(),
            state: SessionState::Initialized,
            database: "zeta4j".to_string(),
            transaction: None,
            bookmarks: BookmarkManager::new(),
            settings: SessionSettings::default(),
            created_at: now,
            last_activity: now,
            metadata: HashMap::new(),
            total_queries: 0,
            total_transactions: 0,
        }
    }

    /// 설정으로 세션 생성
    pub fn with_settings(settings: SessionSettings) -> Self {
        let mut session = Self::new();
        session.settings = settings;
        session
    }

    /// 세션 ID
    pub fn id(&self) -> ServerSessionId {
        self.id
    }

    /// 연결 ID 문자열
    pub fn connection_id(&self) -> String {
        self.id.as_connection_id()
    }

    /// 클라이언트 정보 설정
    pub fn set_client(&mut self, client: ClientInfo) {
        self.client = Some(client);
    }

    /// 클라이언트 정보 조회
    pub fn client(&self) -> Option<&ClientInfo> {
        self.client.as_ref()
    }

    /// 인증 상태
    pub fn auth_state(&self) -> &AuthState {
        &self.auth_state
    }

    /// 인증 상태 (가변)
    pub fn auth_state_mut(&mut self) -> &mut AuthState {
        &mut self.auth_state
    }

    /// 인증 여부
    pub fn is_authenticated(&self) -> bool {
        self.auth_state.is_authenticated()
    }

    /// 세션 상태
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// 세션 상태 설정
    pub fn set_state(&mut self, state: SessionState) {
        self.state = state;
        self.touch();
    }

    /// 현재 데이터베이스
    pub fn database(&self) -> &str {
        &self.database
    }

    /// 데이터베이스 설정
    pub fn set_database(&mut self, database: &str) {
        self.database = database.to_string();
    }

    /// 트랜잭션 진행 중 여부
    pub fn in_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    /// 트랜잭션 컨텍스트
    pub fn transaction(&self) -> Option<&ExtendedTransactionContext> {
        self.transaction.as_ref()
    }

    /// 트랜잭션 컨텍스트 (가변)
    pub fn transaction_mut(&mut self) -> Option<&mut ExtendedTransactionContext> {
        self.transaction.as_mut()
    }

    /// 트랜잭션 시작
    pub fn begin_transaction(&mut self, ctx: ExtendedTransactionContext) {
        self.transaction = Some(ctx);
        self.state = SessionState::InTransaction;
        self.total_transactions += 1;
        self.touch();
    }

    /// 트랜잭션 종료
    pub fn end_transaction(&mut self) -> Option<ExtendedTransactionContext> {
        let tx = self.transaction.take();
        if self.state == SessionState::InTransaction {
            self.state = SessionState::Idle;
        }
        self.touch();
        tx
    }

    /// 북마크 관리자
    pub fn bookmarks(&self) -> &BookmarkManager {
        &self.bookmarks
    }

    /// 북마크 관리자 (가변)
    pub fn bookmarks_mut(&mut self) -> &mut BookmarkManager {
        &mut self.bookmarks
    }

    /// 세션 설정
    pub fn settings(&self) -> &SessionSettings {
        &self.settings
    }

    /// 세션 설정 (가변)
    pub fn settings_mut(&mut self) -> &mut SessionSettings {
        &mut self.settings
    }

    /// 마지막 활동 시간 갱신
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// 유휴 시간
    pub fn idle_duration(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// 세션 수명
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// 세션 타임아웃 체크
    pub fn is_timed_out(&self) -> bool {
        self.idle_duration() > self.settings.session_timeout
    }

    /// 메타데이터 설정
    pub fn set_metadata(&mut self, key: &str, value: impl Into<MetadataValue>) {
        self.metadata.insert(key.to_string(), value.into());
    }

    /// 메타데이터 조회
    pub fn get_metadata(&self, key: &str) -> Option<&MetadataValue> {
        self.metadata.get(key)
    }

    /// 메타데이터 삭제
    pub fn remove_metadata(&mut self, key: &str) -> Option<MetadataValue> {
        self.metadata.remove(key)
    }

    /// 쿼리 수 증가
    pub fn increment_query_count(&mut self) {
        self.total_queries += 1;
        self.touch();
    }

    /// 총 쿼리 수
    pub fn total_queries(&self) -> u64 {
        self.total_queries
    }

    /// 총 트랜잭션 수
    pub fn total_transactions(&self) -> u64 {
        self.total_transactions
    }

    /// 세션 리셋 (RESET 명령)
    pub fn reset(&mut self) {
        self.transaction = None;
        if self.state == SessionState::InTransaction || self.state == SessionState::Active {
            self.state = SessionState::Idle;
        }
        self.touch();
    }

    /// 세션 종료
    pub fn terminate(&mut self) {
        self.state = SessionState::Terminated;
        self.transaction = None;
    }
}

impl Default for ServerSession {
    fn default() -> Self {
        Self::new()
    }
}

/// 세션 스토어 통계
#[derive(Debug, Clone)]
pub struct SessionStoreStats {
    /// 총 세션 수
    pub total_sessions: usize,
    /// 인증된 세션 수
    pub authenticated_sessions: usize,
    /// 활성 세션 수
    pub active_sessions: usize,
    /// 유휴 세션 수
    pub idle_sessions: usize,
    /// 트랜잭션 진행 중인 세션 수
    pub in_transaction_sessions: usize,
    /// 총 생성된 세션 수
    pub sessions_created: u64,
    /// 총 삭제된 세션 수
    pub sessions_removed: u64,
}

/// 세션 스토어 (스레드 안전)
#[derive(Debug)]
pub struct SessionStore {
    sessions: RwLock<HashMap<ServerSessionId, Arc<RwLock<ServerSession>>>>,
    sessions_created: AtomicU64,
    sessions_removed: AtomicU64,
}

impl SessionStore {
    /// 새 세션 스토어 생성
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            sessions_created: AtomicU64::new(0),
            sessions_removed: AtomicU64::new(0),
        }
    }

    /// 세션 생성
    pub fn create_session(&self) -> Arc<RwLock<ServerSession>> {
        let session = ServerSession::new();
        let id = session.id();
        let session = Arc::new(RwLock::new(session));

        self.sessions.write().insert(id, Arc::clone(&session));
        self.sessions_created.fetch_add(1, Ordering::Relaxed);

        session
    }

    /// 설정으로 세션 생성
    pub fn create_session_with_settings(
        &self,
        settings: SessionSettings,
    ) -> Arc<RwLock<ServerSession>> {
        let session = ServerSession::with_settings(settings);
        let id = session.id();
        let session = Arc::new(RwLock::new(session));

        self.sessions.write().insert(id, Arc::clone(&session));
        self.sessions_created.fetch_add(1, Ordering::Relaxed);

        session
    }

    /// 세션 조회
    pub fn get_session(&self, id: ServerSessionId) -> Option<Arc<RwLock<ServerSession>>> {
        self.sessions.read().get(&id).cloned()
    }

    /// 세션 삭제
    pub fn remove_session(&self, id: ServerSessionId) -> Option<Arc<RwLock<ServerSession>>> {
        let session = self.sessions.write().remove(&id);
        if session.is_some() {
            self.sessions_removed.fetch_add(1, Ordering::Relaxed);
        }
        session
    }

    /// 세션 수
    pub fn count(&self) -> usize {
        self.sessions.read().len()
    }

    /// 유휴 세션 정리
    pub fn cleanup_idle(&self, max_idle: Duration) -> usize {
        let mut sessions = self.sessions.write();
        let before = sessions.len();

        sessions.retain(|_, session| {
            let session = session.read();
            session.idle_duration() < max_idle && session.state() != SessionState::Terminated
        });

        let removed = before - sessions.len();
        self.sessions_removed
            .fetch_add(removed as u64, Ordering::Relaxed);
        removed
    }

    /// 타임아웃된 세션 정리
    pub fn cleanup_timed_out(&self) -> usize {
        let mut sessions = self.sessions.write();
        let before = sessions.len();

        sessions.retain(|_, session| {
            let session = session.read();
            !session.is_timed_out() && session.state() != SessionState::Terminated
        });

        let removed = before - sessions.len();
        self.sessions_removed
            .fetch_add(removed as u64, Ordering::Relaxed);
        removed
    }

    /// 모든 세션 ID 조회
    pub fn session_ids(&self) -> Vec<ServerSessionId> {
        self.sessions.read().keys().copied().collect()
    }

    /// 통계
    pub fn stats(&self) -> SessionStoreStats {
        let sessions = self.sessions.read();
        let mut authenticated = 0;
        let mut active = 0;
        let mut idle = 0;
        let mut in_tx = 0;

        for session in sessions.values() {
            let s = session.read();
            if s.is_authenticated() {
                authenticated += 1;
            }
            match s.state() {
                SessionState::Active => active += 1,
                SessionState::Idle => idle += 1,
                SessionState::InTransaction => in_tx += 1,
                _ => {}
            }
        }

        SessionStoreStats {
            total_sessions: sessions.len(),
            authenticated_sessions: authenticated,
            active_sessions: active,
            idle_sessions: idle,
            in_transaction_sessions: in_tx,
            sessions_created: self.sessions_created.load(Ordering::Relaxed),
            sessions_removed: self.sessions_removed.load(Ordering::Relaxed),
        }
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}
