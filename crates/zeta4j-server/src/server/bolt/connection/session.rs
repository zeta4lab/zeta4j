//! Bolt session management.
//!
//! A session represents an authenticated Bolt connection with
//! associated state and transaction context.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;

use crate::server::bolt::handshake::BoltVersion;
use crate::server::bolt::message::AccessMode;
use crate::server::bolt::message::QueryStats;
use crate::server::bolt::packstream::PackStreamValue;
use crate::server::bolt::state::{BoltState, BoltStateMachine};
use zeta4j_credential::SessionId as CredentialSessionId;
use zeta4j_graph::storage::memory::MemoryReservation;

/// Unique session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

impl SessionId {
    /// Create a new session ID.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw ID value.
    pub fn value(&self) -> u64 {
        self.0
    }

    /// Format as connection ID string.
    pub fn as_connection_id(&self) -> String {
        format!("bolt-{}", self.0)
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session-{}", self.0)
    }
}

/// Transaction context within a session.
#[derive(Debug, Clone)]
pub struct TransactionContext {
    /// Transaction ID (from TransactionOrchestrator)
    pub tx_id: Option<u64>,
    /// Bookmarks to wait for
    pub bookmarks: Vec<String>,
    /// Transaction timeout
    pub timeout: Option<Duration>,
    /// Access mode
    pub mode: AccessMode,
    /// Database name
    pub database: Option<String>,
    /// Transaction metadata
    pub metadata: HashMap<String, PackStreamValue>,
    /// Start time
    pub started_at: Instant,
    /// Opaque principal that opened the transaction.
    pub owner_principal_id: Option<String>,
    /// Security generation fixed at transaction creation.
    pub owner_security_generation: Option<u64>,
    /// Canonical database identity fixed at transaction creation.
    pub database_id: Option<String>,
}

impl TransactionContext {
    /// Create a new transaction context.
    pub fn new() -> Self {
        Self {
            tx_id: None,
            bookmarks: Vec::new(),
            timeout: None,
            mode: AccessMode::Write,
            database: None,
            metadata: HashMap::new(),
            started_at: Instant::now(),
            owner_principal_id: None,
            owner_security_generation: None,
            database_id: None,
        }
    }

    /// Check if transaction has timed out.
    pub fn is_timed_out(&self) -> bool {
        if let Some(timeout) = self.timeout {
            self.started_at.elapsed() > timeout
        } else {
            false
        }
    }
}

impl Default for TransactionContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Query context for streaming results.
#[derive(Debug, Clone)]
pub struct QueryContext {
    /// Query ID
    pub qid: i64,
    /// Query string
    pub query: String,
    /// Query parameters
    pub parameters: HashMap<String, PackStreamValue>,
    /// Field names
    pub fields: Vec<String>,
    /// Query result records (stored from RUN, returned in PULL)
    pub records: Vec<Vec<PackStreamValue>>,
    /// Number of records pulled
    pub records_pulled: usize,
    /// Whether there are more records
    pub has_more: bool,
    /// Start time
    pub started_at: Instant,
    /// Query statistics
    pub stats: QueryStats,
    /// Retains admitted query rows until every Bolt record is sent or discarded.
    pub memory_reservation: Option<Arc<MemoryReservation>>,
}

impl QueryContext {
    /// Create a new query context.
    pub fn new(qid: i64, query: &str, parameters: HashMap<String, PackStreamValue>) -> Self {
        Self {
            qid,
            query: query.to_string(),
            parameters,
            fields: Vec::new(),
            records: Vec::new(),
            records_pulled: 0,
            has_more: true,
            started_at: Instant::now(),
            stats: QueryStats::default(),
            memory_reservation: None,
        }
    }

    /// A terminal PULL for an empty result only acknowledges an already executed RUN.
    pub fn requires_current_authorization_for_pull(&self) -> bool {
        !self.records.is_empty()
    }
}

/// Bolt session state.
#[derive(Debug)]
pub struct BoltSession {
    /// Session ID
    id: SessionId,
    /// Protocol version
    version: Option<BoltVersion>,
    /// State machine
    state_machine: BoltStateMachine,
    /// User agent
    user_agent: String,
    /// Authenticated principal (username)
    principal: Option<String>,
    /// Opaque canonical principal ID.
    principal_id: Option<String>,
    /// Persisted security generation installed by HELLO/LOGON.
    security_generation: Option<u64>,
    /// Durable canonical authentication session.
    credential_session_id: Option<CredentialSessionId>,
    /// Whether this Bolt connection created and therefore owns the durable session.
    credential_session_connection_owned: bool,
    /// Current database
    database: String,
    /// Transaction context (if in transaction)
    transaction: Option<TransactionContext>,
    /// Current query context (if streaming)
    query: Option<QueryContext>,
    /// Query ID counter
    next_qid: i64,
    /// Session creation time
    created_at: Instant,
    /// Last activity time
    last_activity: Instant,
    /// Session metadata
    metadata: HashMap<String, PackStreamValue>,
}

impl BoltSession {
    /// Create a new session.
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            id: SessionId::new(),
            version: None,
            state_machine: BoltStateMachine::new(),
            user_agent: String::new(),
            principal: None,
            principal_id: None,
            security_generation: None,
            credential_session_id: None,
            credential_session_connection_owned: false,
            database: "zeta4j".to_string(),
            transaction: None,
            query: None,
            next_qid: 0,
            created_at: now,
            last_activity: now,
            metadata: HashMap::new(),
        }
    }

    /// Get session ID.
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Get connection ID string.
    pub fn connection_id(&self) -> String {
        self.id.as_connection_id()
    }

    /// Get protocol version.
    pub fn version(&self) -> Option<BoltVersion> {
        self.version
    }

    /// Set protocol version.
    pub fn set_version(&mut self, version: BoltVersion) {
        self.version = Some(version);
    }

    /// Get current state.
    pub fn state(&self) -> BoltState {
        self.state_machine.state()
    }

    /// Get reference to state machine.
    pub fn state_machine(&self) -> &BoltStateMachine {
        &self.state_machine
    }

    /// Get mutable reference to state machine.
    pub fn state_machine_mut(&mut self) -> &mut BoltStateMachine {
        &mut self.state_machine
    }

    /// Get user agent.
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Set user agent.
    pub fn set_user_agent(&mut self, agent: &str) {
        self.user_agent = agent.to_string();
    }

    /// Get authenticated principal.
    pub fn principal(&self) -> Option<&str> {
        self.principal.as_deref()
    }

    /// Set authenticated principal.
    pub fn set_principal(&mut self, principal: &str, principal_id: &str, generation: u64) {
        self.principal = Some(principal.to_string());
        self.principal_id = Some(principal_id.to_string());
        self.security_generation = Some(generation);
    }

    pub fn set_authenticated_session(
        &mut self,
        principal: &str,
        principal_id: &str,
        generation: u64,
        credential_session_id: CredentialSessionId,
        connection_owned: bool,
    ) -> Option<(CredentialSessionId, bool)> {
        self.set_principal(principal, principal_id, generation);
        let replaced = self
            .credential_session_id
            .replace(credential_session_id)
            .map(|session_id| (session_id, self.credential_session_connection_owned));
        self.credential_session_connection_owned = connection_owned;
        replaced
    }

    /// Get the installed opaque principal ID and generation.
    pub fn principal_identity(&self) -> Option<(&str, u64)> {
        Some((self.principal_id.as_deref()?, self.security_generation?))
    }

    pub fn credential_session_id(&self) -> Option<&CredentialSessionId> {
        self.credential_session_id.as_ref()
    }

    /// Clear all authentication state after LOGOFF.
    pub fn clear_principal(&mut self) -> Option<(CredentialSessionId, bool)> {
        self.principal = None;
        self.principal_id = None;
        self.security_generation = None;
        let cleared = self
            .credential_session_id
            .take()
            .map(|session_id| (session_id, self.credential_session_connection_owned));
        self.credential_session_connection_owned = false;
        cleared
    }

    /// Get current database.
    pub fn database(&self) -> &str {
        &self.database
    }

    /// Set current database.
    pub fn set_database(&mut self, db: &str) {
        self.database = db.to_string();
    }

    /// Check if in transaction.
    pub fn in_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    /// Get transaction context.
    pub fn transaction(&self) -> Option<&TransactionContext> {
        self.transaction.as_ref()
    }

    /// Start a new transaction.
    pub fn begin_transaction(&mut self, ctx: TransactionContext) {
        self.transaction = Some(ctx);
    }

    /// End the current transaction.
    pub fn end_transaction(&mut self) -> Option<TransactionContext> {
        self.transaction.take()
    }

    /// Check if streaming results.
    pub fn is_streaming(&self) -> bool {
        self.query.is_some()
    }

    /// Get query context.
    pub fn query(&self) -> Option<&QueryContext> {
        self.query.as_ref()
    }

    /// Get mutable query context.
    pub fn query_mut(&mut self) -> Option<&mut QueryContext> {
        self.query.as_mut()
    }

    /// Start a new query.
    pub fn begin_query(
        &mut self,
        query: &str,
        parameters: HashMap<String, PackStreamValue>,
    ) -> i64 {
        let qid = self.next_qid;
        self.next_qid += 1;
        self.query = Some(QueryContext::new(qid, query, parameters));
        qid
    }

    /// End the current query.
    pub fn end_query(&mut self) -> Option<QueryContext> {
        self.query.take()
    }

    /// Update last activity time.
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Get idle duration.
    pub fn idle_duration(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// Get session age.
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Set session metadata.
    pub fn set_metadata(&mut self, key: &str, value: PackStreamValue) {
        self.metadata.insert(key.to_string(), value);
    }

    /// Get session metadata.
    pub fn get_metadata(&self, key: &str) -> Option<&PackStreamValue> {
        self.metadata.get(key)
    }

    /// Reset session state (for RESET message).
    pub fn reset(&mut self) {
        self.transaction = None;
        self.query = None;
    }
}

impl Default for BoltSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logoff_clear_removes_all_cached_authorization_identity() {
        let mut session = BoltSession::new();
        session.set_principal("alice", "user.opaque", 7);
        assert_eq!(session.principal(), Some("alice"));
        assert_eq!(session.principal_identity(), Some(("user.opaque", 7)));

        session.clear_principal();
        assert_eq!(session.principal(), None);
        assert_eq!(session.principal_identity(), None);
    }

    #[test]
    fn credential_session_tracks_connection_ownership() {
        let mut session = BoltSession::new();
        let bearer_session = CredentialSessionId::new();
        assert_eq!(
            session.set_authenticated_session(
                "alice",
                "user.opaque",
                7,
                bearer_session.clone(),
                false,
            ),
            None
        );
        assert_eq!(session.clear_principal(), Some((bearer_session, false)));

        let password_session = CredentialSessionId::new();
        assert_eq!(
            session.set_authenticated_session(
                "alice",
                "user.opaque",
                7,
                password_session.clone(),
                true,
            ),
            None
        );
        assert_eq!(session.clear_principal(), Some((password_session, true)));
    }

    #[test]
    fn transaction_context_keeps_owner_generation_and_database_identity() {
        let mut context = TransactionContext::new();
        context.owner_principal_id = Some("user.opaque".to_string());
        context.owner_security_generation = Some(7);
        context.database_id = Some("database.opaque".to_string());
        let mut session = BoltSession::new();
        session.begin_transaction(context);

        let transaction = session.transaction();
        assert!(transaction.is_some_and(|transaction| {
            transaction.owner_principal_id.as_deref() == Some("user.opaque")
                && transaction.owner_security_generation == Some(7)
                && transaction.database_id.as_deref() == Some("database.opaque")
        }));
    }

    #[test]
    fn empty_query_result_can_finish_after_its_principal_generation_changes() {
        let mut context = QueryContext::new(1, "GRANT ROLE reader TO alice", HashMap::new());
        assert!(!context.requires_current_authorization_for_pull());

        context.records.push(vec![PackStreamValue::Integer(1)]);
        assert!(context.requires_current_authorization_for_pull());
    }
}

/// Thread-safe session store.
#[derive(Debug, Default)]
pub struct SessionStore {
    sessions: RwLock<HashMap<SessionId, Arc<RwLock<BoltSession>>>>,
}

impl SessionStore {
    /// Create a new session store.
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Create and store a new session.
    pub fn create_session(&self) -> Arc<RwLock<BoltSession>> {
        let session = BoltSession::new();
        let id = session.id();
        let session = Arc::new(RwLock::new(session));
        self.sessions.write().insert(id, Arc::clone(&session));
        session
    }

    /// Get a session by ID.
    pub fn get_session(&self, id: SessionId) -> Option<Arc<RwLock<BoltSession>>> {
        self.sessions.read().get(&id).cloned()
    }

    /// Remove a session.
    pub fn remove_session(&self, id: SessionId) -> Option<Arc<RwLock<BoltSession>>> {
        self.sessions.write().remove(&id)
    }

    /// Get session count.
    pub fn count(&self) -> usize {
        self.sessions.read().len()
    }

    /// Clean up idle sessions.
    pub fn cleanup_idle(&self, max_idle: Duration) -> usize {
        let mut sessions = self.sessions.write();
        let before = sessions.len();

        sessions.retain(|_, session| {
            let session = session.read();
            session.idle_duration() < max_idle
        });

        before - sessions.len()
    }
}
