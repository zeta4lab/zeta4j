// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bolt connection handler.
//!
//! Handles the lifecycle of a Bolt connection including handshake,
//! authentication, and message processing.
//!
//! v1.79.0: AsyncQueryExecutor trait for fully async query execution (block_on removed).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use parking_lot::RwLock;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;
use tracing::{debug, error, info};

use super::session::{BoltSession, QueryContext, TransactionContext};
use crate::server::access::{InstalledExecutionIdentity, PublicError, authorize_cypher_execution};
use crate::server::auth::CredentialTokenService;
use crate::server::bolt::codec::BoltRequestCodec;
use crate::server::bolt::handshake::{BoltVersion, Handshake};
use crate::server::bolt::message::*;
use crate::server::bolt::packstream::PackStreamValue;
use crate::server::bolt::state::{BoltEvent, BoltState};
use crate::server::bolt::{BoltConfig, BoltError, BoltResult};
use crate::server::system::{LocalSecurityService, SecurityManager};
use crate::server::tenant::TenantDatabaseManager;
use zeta4j_credential::{CredentialSubjectId, SessionId as CredentialSessionId};
use zeta4j_graph::storage::memory::MemoryReservation;

/// Server identification string (version from Cargo.toml).
const SERVER_AGENT: &str = concat!("Zeta4j/", env!("CARGO_PKG_VERSION"));

/// Bolt connection handler.
struct AuthenticatedIdentity {
    username: String,
    principal_id: String,
    security_generation: u64,
    credential_session_id: CredentialSessionId,
    connection_owned: bool,
}

pub struct BoltConnection<S> {
    /// Underlying stream
    stream: Option<Framed<S, BoltRequestCodec>>,
    /// Session state
    session: Arc<RwLock<BoltSession>>,
    /// Server configuration
    config: Arc<BoltConfig>,
    /// Query executor callback (async)
    query_executor: Option<Arc<dyn AsyncQueryExecutor>>,
    /// Security manager for authentication (RocksDB-backed)
    security_manager: Option<Arc<SecurityManager>>,
    /// Canonical durable credential lifecycle.
    credential_security: Option<Arc<LocalSecurityService>>,
    credential_tokens: Option<Arc<CredentialTokenService>>,
    client_address: Option<String>,
    /// Canonical database identity resolver.
    tenant_db_manager: Option<Arc<TenantDatabaseManager>>,
}

impl<S> BoltConnection<S> {
    /// Closes the durable credential session this connection opened, if any.
    /// Idempotent: the session is detached before it is closed.
    fn close_installed_session(&self) {
        let session = self.session.write().clear_principal();
        if let (Some(credential_security), Some((session_id, true))) =
            (&self.credential_security, session)
            && let Err(error) = credential_security.close_password_session(&session_id)
        {
            debug!("Failed to close Bolt credential session: {}", error);
        }
    }
}

/// Every exit path — GOODBYE, client disconnect, idle timeout, protocol error
/// or a cancelled connection future — releases the durable session.
impl<S> Drop for BoltConnection<S> {
    fn drop(&mut self) {
        self.close_installed_session();
    }
}

/// Async trait for query execution (Bolt protocol).
///
/// v1.79.0: Changed from sync to async trait to eliminate block_on usage.
/// v1.80.0: Added username parameter for Admin command authorization (#166).
#[async_trait]
pub trait AsyncQueryExecutor: Send + Sync {
    /// Execute a query and return results.
    ///
    /// # Arguments
    /// * `query` - Cypher 쿼리 문자열
    /// * `parameters` - 쿼리 파라미터
    /// * `database` - 대상 데이터베이스
    /// * `tx_id` - 트랜잭션 ID (명시적 트랜잭션인 경우)
    /// * `principal` - 인증 시점에 설치된 정규 주체 신원
    async fn execute(
        &self,
        query: &str,
        parameters: &HashMap<String, PackStreamValue>,
        database: &str,
        tx_id: Option<u64>,
        principal: Option<&ExecutionPrincipal>,
    ) -> BoltResult<QueryResult>;

    /// Begin a transaction.
    async fn begin_transaction(&self, database: &str) -> BoltResult<u64>;

    /// Commit a transaction.
    async fn commit_transaction(&self, tx_id: u64) -> BoltResult<String>;

    /// Rollback a transaction.
    async fn rollback_transaction(&self, tx_id: u64) -> BoltResult<()>;
}

/// Canonical authenticated identity carried through every query executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPrincipal {
    pub username: String,
    pub principal_id: String,
    pub security_generation: u64,
}

/// Query execution result.
#[derive(Debug)]
pub struct QueryResult {
    /// Field names
    pub fields: Vec<String>,
    /// Result records
    pub records: Vec<Vec<PackStreamValue>>,
    /// Query statistics
    pub stats: QueryStats,
    /// Bookmark
    pub bookmark: Option<String>,
    /// Retains admitted source rows through Bolt record streaming.
    pub memory_reservation: Option<Arc<MemoryReservation>>,
}

impl<S> BoltConnection<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    /// Create a new connection handler.
    pub fn new(stream: S, config: Arc<BoltConfig>, session: Arc<RwLock<BoltSession>>) -> Self {
        Self {
            stream: Some(Framed::new(stream, BoltRequestCodec::new())),
            session,
            config,
            query_executor: None,
            security_manager: None,
            credential_security: None,
            credential_tokens: None,
            client_address: None,
            tenant_db_manager: None,
        }
    }

    /// Set the query executor.
    pub fn with_executor(mut self, executor: Arc<dyn AsyncQueryExecutor>) -> Self {
        self.query_executor = Some(executor);
        self
    }

    /// Set the query executor from a boxed trait object.
    ///
    /// Convenience method that converts Box to Arc.
    pub fn with_boxed_executor(mut self, executor: Box<dyn AsyncQueryExecutor>) -> Self {
        self.query_executor = Some(Arc::from(executor));
        self
    }

    /// Set the security manager for authentication (RocksDB-backed).
    pub fn with_security_manager(mut self, security_manager: Arc<SecurityManager>) -> Self {
        self.security_manager = Some(security_manager);
        self
    }

    pub fn with_credential_security(
        mut self,
        credential_security: Arc<LocalSecurityService>,
    ) -> Self {
        self.credential_security = Some(credential_security);
        self
    }

    pub fn with_credential_tokens(
        mut self,
        credential_tokens: Arc<CredentialTokenService>,
    ) -> Self {
        self.credential_tokens = Some(credential_tokens);
        self
    }

    pub fn with_client_address(mut self, client_address: impl Into<String>) -> Self {
        self.client_address = Some(client_address.into());
        self
    }

    /// Set the canonical database identity resolver.
    pub fn with_tenant_db_manager(mut self, manager: Arc<TenantDatabaseManager>) -> Self {
        self.tenant_db_manager = Some(manager);
        self
    }

    /// Perform handshake negotiation.
    pub async fn handshake(&mut self, handshake_bytes: &[u8; 20]) -> BoltResult<BoltVersion> {
        // Create handshake handler with our supported versions
        let handshake = Handshake::with_versions(self.config.supported_versions.clone());

        // Transition state
        {
            let mut session = self.session.write();
            session
                .state_machine_mut()
                .transition(BoltEvent::HandshakeReceived)?;
        }

        // Process handshake and negotiate version
        let result = handshake
            .process(handshake_bytes)
            .map_err(BoltError::Handshake)?;
        let version = result.version;

        // Update session
        {
            let mut session = self.session.write();
            session.set_version(version);
            session
                .state_machine_mut()
                .transition(BoltEvent::VersionNegotiated(version))?;
        }

        debug!("Negotiated Bolt version: {:?}", version);
        Ok(version)
    }

    /// Run the connection message loop.
    pub async fn run(&mut self) -> BoltResult<()> {
        let mut framed = self
            .stream
            .take()
            .ok_or_else(|| BoltError::Protocol("Stream already taken".to_string()))?;

        loop {
            // Check if connection should close
            let state = self.session.read().state();
            if state == BoltState::Disconnected || state == BoltState::Defunct {
                break;
            }

            // Read next message
            let next = tokio::time::timeout(self.config.connection_timeout, framed.next())
                .await
                .map_err(|_| BoltError::Timeout)?;
            let request = match next {
                Some(Ok(req)) => req,
                Some(Err(e)) => {
                    error!("Error reading message: {}", e);
                    self.send_failure(&mut framed, &e).await?;
                    continue;
                }
                None => {
                    debug!("Connection closed by client");
                    break;
                }
            };

            // Update activity
            self.session.write().touch();

            // Process message
            let response = self.process_request(&request).await;

            // Send response(s)
            match response {
                Ok(responses) => {
                    for resp in responses {
                        match tokio::time::timeout(
                            self.config.connection_timeout,
                            framed.send(resp),
                        )
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                error!("Error sending response: {}", error);
                                return Err(error);
                            }
                            Err(_) => return Err(BoltError::Timeout),
                        }
                    }
                    if matches!(request, BoltRequest::Pull(_))
                        && self
                            .session
                            .read()
                            .query()
                            .is_some_and(|query| !query.has_more)
                    {
                        self.session.write().end_query();
                    }
                }
                Err(e) => {
                    self.send_failure(&mut framed, &e).await?;
                }
            }

            // Handle GOODBYE
            if matches!(request, BoltRequest::Goodbye) {
                break;
            }
        }

        Ok(())
    }

    /// Process a request and return response(s).
    async fn process_request(&mut self, request: &BoltRequest) -> BoltResult<Vec<BoltResponse>> {
        debug!("Processing {}", request.name());

        // Check if message can be accepted in current state
        let can_accept = {
            let session = self.session.read();
            session.state_machine().can_accept(request)
        };
        if !can_accept {
            let state = self.session.read().state();
            if state == BoltState::Failed {
                // In FAILED state, only RESET is accepted
                if !matches!(request, BoltRequest::Reset) {
                    return Ok(vec![BoltResponse::Ignored]);
                }
            } else {
                return Err(BoltError::InvalidState(format!(
                    "Cannot accept {} in {} state",
                    request.name(),
                    state.name()
                )));
            }
        }

        if self.config.auth_required
            && matches!(
                request,
                BoltRequest::Run(_)
                    | BoltRequest::Pull(_)
                    | BoltRequest::Discard(_)
                    | BoltRequest::Begin(_)
                    | BoltRequest::Commit
                    | BoltRequest::Rollback
                    | BoltRequest::Route(_)
            )
        {
            self.validate_installed_session()
                .map_err(Self::access_error)?;
        }

        match request {
            BoltRequest::Hello(msg) => self.handle_hello(msg).await,
            BoltRequest::Goodbye => self.handle_goodbye().await,
            BoltRequest::Reset => self.handle_reset().await,
            BoltRequest::Run(msg) => self.handle_run(msg).await,
            BoltRequest::Pull(msg) => self.handle_pull(msg).await,
            BoltRequest::Discard(msg) => self.handle_discard(msg).await,
            BoltRequest::Begin(msg) => self.handle_begin(msg).await,
            BoltRequest::Commit => self.handle_commit().await,
            BoltRequest::Rollback => self.handle_rollback().await,
            BoltRequest::Route(msg) => self.handle_route(msg).await,
            BoltRequest::Logon(msg) => self.handle_logon(msg).await,
            BoltRequest::Logoff => self.handle_logoff().await,
        }
    }

    /// Handle HELLO message.
    async fn handle_hello(&mut self, msg: &HelloMessage) -> BoltResult<Vec<BoltResponse>> {
        // Store user agent
        {
            let mut session = self.session.write();
            session.set_user_agent(&msg.user_agent);
        }

        let mut authenticated = None;
        if self.config.auth_required {
            let auth_result = match msg.auth.as_ref() {
                Some(auth) => self.verify_authentication(auth).await,
                None => Err(PublicError::AuthenticationFailed),
            };
            match auth_result {
                Ok(identity) => authenticated = Some(identity),
                Err(error) => {
                    // Authentication failed
                    let mut session = self.session.write();
                    session
                        .state_machine_mut()
                        .transition(BoltEvent::HelloFailure)?;
                    let code = error.code();
                    let failure = FailureMessage::new(&code, "Authentication failed");
                    return Ok(vec![BoltResponse::Failure(failure)]);
                }
            }
        } else if let Some(auth) = &msg.auth
            && self.credential_security.is_some()
            && let Ok(identity) = self.verify_authentication(auth).await
        {
            authenticated = Some(identity);
        }

        if let Some(identity) = authenticated {
            let mut session = self.session.write();
            session.set_authenticated_session(
                &identity.username,
                &identity.principal_id,
                identity.security_generation,
                identity.credential_session_id,
                identity.connection_owned,
            );
        }

        // Transition state
        {
            let mut session = self.session.write();
            session
                .state_machine_mut()
                .transition(BoltEvent::HelloSuccess)?;
        }

        let connection_id = self.session.read().connection_id();
        let response = SuccessMessage::hello_success(SERVER_AGENT, &connection_id);

        info!("HELLO from {} ({})", msg.user_agent, connection_id);

        Ok(vec![BoltResponse::Success(response)])
    }

    /// Verify authentication credentials.
    async fn verify_authentication(
        &self,
        auth: &AuthToken,
    ) -> Result<AuthenticatedIdentity, PublicError> {
        if auth.scheme == "bearer" {
            let credentials = auth
                .credentials
                .as_ref()
                .ok_or(PublicError::AuthenticationFailed)?;
            let credential_security = self
                .credential_security
                .as_ref()
                .ok_or(PublicError::PolicyInvariant)?;
            let credential_tokens = self
                .credential_tokens
                .as_ref()
                .ok_or(PublicError::PolicyInvariant)?;
            let (claims, user) = credential_security
                .validate_access(
                    credential_tokens,
                    &zeta4j_credential::CredentialSecret::new(credentials),
                    zeta4j_credential::CredentialAudience::Bolt,
                )
                .map_err(|_| PublicError::AuthenticationFailed)?;
            return Ok(AuthenticatedIdentity {
                username: user.username,
                principal_id: user.principal_id.as_str().to_string(),
                security_generation: user.security_generation,
                credential_session_id: claims.session_id,
                connection_owned: false,
            });
        }
        if auth.scheme != "basic" {
            return Err(PublicError::AuthenticationFailed);
        }
        let principal = auth
            .principal
            .as_ref()
            .ok_or(PublicError::AuthenticationFailed)?;

        let credentials = auth
            .credentials
            .as_ref()
            .ok_or(PublicError::AuthenticationFailed)?;

        let credential_security = self
            .credential_security
            .as_ref()
            .ok_or(PublicError::PolicyInvariant)?;
        let client_address = self.client_address.as_deref().unwrap_or("anonymous");
        let authenticated = credential_security
            .open_password_session_from_client(principal, credentials, client_address)
            .map_err(|_| PublicError::AuthenticationFailed)?;
        info!("Bolt authentication successful");
        Ok(AuthenticatedIdentity {
            username: authenticated.user.username,
            principal_id: authenticated.user.principal_id.as_str().to_string(),
            security_generation: authenticated.user.security_generation,
            credential_session_id: authenticated.session_id,
            connection_owned: true,
        })
    }

    fn validate_installed_session(&self) -> Result<(), PublicError> {
        let credential_security = self
            .credential_security
            .as_ref()
            .ok_or(PublicError::PolicyInvariant)?;
        let (session_id, subject, generation) = {
            let session = self.session.read();
            let session_id = session
                .credential_session_id()
                .cloned()
                .ok_or(PublicError::AuthenticationFailed)?;
            let (subject, generation) = session
                .principal_identity()
                .ok_or(PublicError::AuthenticationFailed)?;
            (session_id, subject.to_string(), generation)
        };
        let subject =
            CredentialSubjectId::parse(subject).map_err(|_| PublicError::AuthenticationFailed)?;
        credential_security
            .validate_password_session(&session_id, &subject, generation)
            .map_err(|_| PublicError::PrincipalInactive)
    }

    fn access_error(error: PublicError) -> BoltError {
        BoltError::QueryExecution {
            code: error.code(),
            message: match error {
                PublicError::AuthenticationFailed
                | PublicError::TokenExpired
                | PublicError::PrincipalInactive => "Authentication failed",
                PublicError::DatabaseNotFound => "Database not found",
                PublicError::PolicyInvariant => "Access check unavailable",
            }
            .to_string(),
        }
    }

    /// Re-resolves the installed principal and admits it to one existing database.
    async fn admit_database(&self, database: &str) -> BoltResult<()> {
        if !self.config.auth_required {
            return Ok(());
        }
        let security_manager = self
            .security_manager
            .as_ref()
            .ok_or_else(|| Self::access_error(PublicError::PolicyInvariant))?;
        let database_manager = self
            .tenant_db_manager
            .as_ref()
            .ok_or_else(|| Self::access_error(PublicError::PolicyInvariant))?;
        let (username, principal_id, security_generation) = {
            let session = self.session.read();
            let username = session
                .principal()
                .ok_or_else(|| Self::access_error(PublicError::AuthenticationFailed))?;
            let (principal_id, security_generation) = session
                .principal_identity()
                .ok_or_else(|| Self::access_error(PublicError::AuthenticationFailed))?;
            (
                username.to_string(),
                principal_id.to_string(),
                security_generation,
            )
        };
        authorize_cypher_execution(
            security_manager,
            database_manager,
            InstalledExecutionIdentity {
                username: &username,
                principal_id: &principal_id,
                security_generation,
            },
            database,
        )
        .await
        .map_err(Self::access_error)
    }

    async fn verify_transaction_owner(
        &self,
        transaction: &TransactionContext,
        database: &str,
    ) -> BoltResult<()> {
        if !self.config.auth_required {
            return Ok(());
        }
        let (principal_id, generation) = self
            .session
            .read()
            .principal_identity()
            .map(|(id, generation)| (id.to_string(), generation))
            .ok_or_else(|| Self::access_error(PublicError::AuthenticationFailed))?;
        let manager = self
            .tenant_db_manager
            .as_ref()
            .ok_or_else(|| Self::access_error(PublicError::PolicyInvariant))?;
        let canonical_database = manager
            .get(database)
            .await
            .map_err(|_| Self::access_error(PublicError::DatabaseNotFound))?;
        let database_id = format!("database.{}", canonical_database.id);
        if transaction.owner_principal_id.as_deref() != Some(principal_id.as_str())
            || transaction.owner_security_generation != Some(generation)
            || transaction.database_id.as_deref() != Some(database_id.as_str())
        {
            // The transaction belongs to another principal generation or database.
            return Err(Self::access_error(PublicError::PrincipalInactive));
        }
        Ok(())
    }

    /// Handle GOODBYE message.
    async fn handle_goodbye(&mut self) -> BoltResult<Vec<BoltResponse>> {
        self.close_installed_session();
        {
            let mut session = self.session.write();
            session.state_machine_mut().transition(BoltEvent::Goodbye)?;
        }

        debug!("GOODBYE received");
        // No response for GOODBYE
        Ok(vec![])
    }

    /// Handle RESET message.
    async fn handle_reset(&mut self) -> BoltResult<Vec<BoltResponse>> {
        // Rollback any active transaction
        let tx_to_rollback = self
            .session
            .write()
            .end_transaction()
            .and_then(|tx| tx.tx_id);

        if let (Some(executor), Some(tx_id)) = (&self.query_executor, tx_to_rollback) {
            let _ = executor.rollback_transaction(tx_id).await;
        }

        // End any active query
        self.session.write().end_query();

        // Reset session
        self.session.write().reset();

        // Transition state
        {
            let mut session = self.session.write();
            session.state_machine_mut().transition(BoltEvent::Reset)?;
        }

        debug!("RESET completed");
        Ok(vec![BoltResponse::Success(SuccessMessage::new())])
    }

    /// Handle RUN message.
    ///
    /// v1.79.0: Uses async executor directly (spawn_blocking removed).
    /// v1.80.0: Passes authenticated username for Admin command authorization (#166).
    async fn handle_run(&mut self, msg: &RunMessage) -> BoltResult<Vec<BoltResponse>> {
        let (database, principal) = {
            let session = self.session.read();
            let db = msg
                .extra
                .get("db")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| session.database().to_string());
            let principal = session.principal().zip(session.principal_identity()).map(
                |(username, (principal_id, security_generation))| ExecutionPrincipal {
                    username: username.to_string(),
                    principal_id: principal_id.to_string(),
                    security_generation,
                },
            );
            (db, principal)
        };
        self.admit_database(&database).await?;
        let transaction = { self.session.read().transaction().cloned() };
        if let Some(transaction) = transaction {
            self.verify_transaction_owner(&transaction, &database)
                .await?;
        }

        // Get transaction ID if in transaction
        let tx_id = self.session.read().transaction().and_then(|tx| tx.tx_id);

        // Execute query using async executor directly
        let result = if let Some(ref executor) = self.query_executor {
            executor
                .execute(
                    &msg.query,
                    &msg.parameters,
                    &database,
                    tx_id,
                    principal.as_ref(),
                )
                .await?
        } else {
            // No executor - return empty result
            QueryResult {
                fields: vec![],
                records: vec![],
                stats: QueryStats::default(),
                bookmark: None,
                memory_reservation: None,
            }
        };

        // Start query context
        let qid = self
            .session
            .write()
            .begin_query(&msg.query, msg.parameters.clone());

        // Update query context with fields, records, and stats
        if let Some(query_ctx) = self.session.write().query_mut() {
            query_ctx.fields = result.fields.clone();
            query_ctx.records = result.records;
            query_ctx.has_more = !query_ctx.records.is_empty();
            query_ctx.stats = result.stats;
            query_ctx.memory_reservation = result.memory_reservation;
        }

        // Transition state
        {
            let mut session = self.session.write();
            session
                .state_machine_mut()
                .transition(BoltEvent::RunSuccess)?;
        }

        let response = SuccessMessage::run_success(result.fields, Some(qid));
        debug!("RUN completed");

        Ok(vec![BoltResponse::Success(response)])
    }

    /// Handle PULL message.
    async fn handle_pull(&mut self, msg: &PullMessage) -> BoltResult<Vec<BoltResponse>> {
        let database = self.session.read().database().to_string();
        let streams_records = self
            .session
            .read()
            .query()
            .is_none_or(QueryContext::requires_current_authorization_for_pull);
        if streams_records {
            self.admit_database(&database).await?;
        }
        let mut responses = Vec::new();

        // Get records and stats from query context
        let (records_to_send, has_more, stats) = {
            let mut session = self.session.write();
            if let Some(query_ctx) = session.query_mut() {
                // Determine how many records to send (n=-1 means all)
                let n = msg.n;
                let remaining = query_ctx.records.len() - query_ctx.records_pulled;

                let count = if n < 0 {
                    remaining // Send all remaining
                } else {
                    std::cmp::min(n as usize, remaining)
                };

                // Collect records to send
                let start = query_ctx.records_pulled;
                let end = start + count;
                let records: Vec<Vec<PackStreamValue>> = query_ctx.records[start..end].to_vec();

                // Update pulled count
                query_ctx.records_pulled = end;
                query_ctx.has_more = end < query_ctx.records.len();

                // Get stats to include in final response
                let stats = query_ctx.stats.clone();

                (records, query_ctx.has_more, stats)
            } else {
                (Vec::new(), false, QueryStats::default())
            }
        };

        // Create RECORD messages for each row
        for record in records_to_send {
            responses.push(BoltResponse::Record(RecordMessage::new(record)));
        }

        let bookmark = if !has_more && !self.session.read().in_transaction() {
            Some(format!("bm:{}", self.session.read().id().value()))
        } else {
            None
        };

        // Transition state
        let event = if has_more {
            BoltEvent::PullMore
        } else {
            BoltEvent::PullComplete
        };
        {
            let mut session = self.session.write();
            session.state_machine_mut().transition(event)?;
        }

        // Include stats in the final SUCCESS message
        responses.push(BoltResponse::Success(
            SuccessMessage::streaming_success_with_stats(has_more, bookmark, Some(&stats)),
        ));

        debug!(
            "PULL completed (sent {} records, has_more={})",
            responses.len() - 1,
            has_more
        );
        Ok(responses)
    }

    /// Handle DISCARD message.
    async fn handle_discard(&mut self, _msg: &DiscardMessage) -> BoltResult<Vec<BoltResponse>> {
        let database = self.session.read().database().to_string();
        self.admit_database(&database).await?;
        // Discard remaining results
        self.session.write().end_query();

        // Transition state
        {
            let mut session = self.session.write();
            session
                .state_machine_mut()
                .transition(BoltEvent::DiscardComplete)?;
        }

        let has_more = false;
        debug!("DISCARD completed");

        Ok(vec![BoltResponse::Success(
            SuccessMessage::streaming_success(has_more, None),
        )])
    }

    /// Handle BEGIN message.
    async fn handle_begin(&mut self, msg: &BeginMessage) -> BoltResult<Vec<BoltResponse>> {
        let database = msg
            .database
            .clone()
            .unwrap_or_else(|| self.session.read().database().to_string());
        self.admit_database(&database).await?;

        // Create transaction context
        let mut ctx = TransactionContext::new();
        ctx.bookmarks = msg.bookmarks.clone();
        ctx.timeout = msg.tx_timeout;
        ctx.mode = msg.mode;
        ctx.database = Some(database.clone());
        ctx.metadata = msg.tx_metadata.clone();
        if self.config.auth_required {
            let (principal_id, generation) = self
                .session
                .read()
                .principal_identity()
                .map(|(id, generation)| (id.to_string(), generation))
                .ok_or_else(|| Self::access_error(PublicError::AuthenticationFailed))?;
            let manager = self
                .tenant_db_manager
                .as_ref()
                .ok_or_else(|| Self::access_error(PublicError::PolicyInvariant))?;
            let canonical_database = manager
                .get(&database)
                .await
                .map_err(|_| Self::access_error(PublicError::DatabaseNotFound))?;
            ctx.owner_principal_id = Some(principal_id);
            ctx.owner_security_generation = Some(generation);
            ctx.database_id = Some(format!("database.{}", canonical_database.id));
        }

        // Begin transaction with executor (async)
        if let Some(ref executor) = self.query_executor {
            let tx_id = executor.begin_transaction(&database).await?;
            ctx.tx_id = Some(tx_id);
        }

        self.session.write().begin_transaction(ctx);

        // Transition state
        {
            let mut session = self.session.write();
            session
                .state_machine_mut()
                .transition(BoltEvent::BeginSuccess)?;
        }

        debug!("BEGIN transaction");
        Ok(vec![BoltResponse::Success(SuccessMessage::begin_success())])
    }

    /// Handle COMMIT message.
    async fn handle_commit(&mut self) -> BoltResult<Vec<BoltResponse>> {
        let tx = self
            .session
            .read()
            .transaction()
            .cloned()
            .ok_or_else(|| BoltError::InvalidState("No active transaction".to_string()))?;
        let database = tx
            .database
            .clone()
            .unwrap_or_else(|| self.session.read().database().to_string());
        self.admit_database(&database).await?;
        self.verify_transaction_owner(&tx, &database).await?;
        let tx = self
            .session
            .write()
            .end_transaction()
            .ok_or_else(|| BoltError::InvalidState("No active transaction".to_string()))?;

        let bookmark = if let (Some(executor), Some(tx_id)) = (&self.query_executor, tx.tx_id) {
            executor.commit_transaction(tx_id).await?
        } else {
            format!("bm:{}", self.session.read().id().value())
        };

        // Transition state
        {
            let mut session = self.session.write();
            session
                .state_machine_mut()
                .transition(BoltEvent::CommitSuccess)?;
        }

        debug!("COMMIT transaction");
        Ok(vec![BoltResponse::Success(SuccessMessage::commit_success(
            bookmark,
        ))])
    }

    /// Handle ROLLBACK message.
    async fn handle_rollback(&mut self) -> BoltResult<Vec<BoltResponse>> {
        let tx = self
            .session
            .read()
            .transaction()
            .cloned()
            .ok_or_else(|| BoltError::InvalidState("No active transaction".to_string()))?;
        let database = tx
            .database
            .clone()
            .unwrap_or_else(|| self.session.read().database().to_string());
        self.admit_database(&database).await?;
        self.verify_transaction_owner(&tx, &database).await?;
        let tx = self
            .session
            .write()
            .end_transaction()
            .ok_or_else(|| BoltError::InvalidState("No active transaction".to_string()))?;

        if let (Some(executor), Some(tx_id)) = (&self.query_executor, tx.tx_id) {
            executor.rollback_transaction(tx_id).await?;
        }

        // Transition state
        {
            let mut session = self.session.write();
            session
                .state_machine_mut()
                .transition(BoltEvent::RollbackSuccess)?;
        }

        debug!("ROLLBACK transaction");
        Ok(vec![BoltResponse::Success(
            SuccessMessage::rollback_success(),
        )])
    }

    /// Handle ROUTE message.
    async fn handle_route(&mut self, _msg: &RouteMessage) -> BoltResult<Vec<BoltResponse>> {
        let database = self.session.read().database().to_string();
        self.admit_database(&database).await?;
        // Single-node deployment - return self as the only server
        let mut table = RoutingTable::new("zeta4j", 300);

        // Get actual bind address from config
        let bind_addr = self.config.bind_addr;
        let server_address = format!("{}:{}", bind_addr.ip(), bind_addr.port());

        table.add_server(ServerRole::Write, vec![server_address.clone()]);
        table.add_server(ServerRole::Read, vec![server_address.clone()]);
        table.add_server(ServerRole::Route, vec![server_address]);

        let mut response = SuccessMessage::new();
        response.add("rt", PackStreamValue::Map(table.to_map()));

        debug!("ROUTE completed");
        Ok(vec![BoltResponse::Success(response)])
    }

    /// Handle LOGON message (re-authentication).
    async fn handle_logon(&mut self, msg: &LogonMessage) -> BoltResult<Vec<BoltResponse>> {
        let identity = self
            .verify_authentication(&msg.auth)
            .await
            .map_err(Self::access_error)?;
        let replaced = self.session.write().set_authenticated_session(
            &identity.username,
            &identity.principal_id,
            identity.security_generation,
            identity.credential_session_id.clone(),
            identity.connection_owned,
        );
        if let (Some(credential_security), Some((replaced, true))) =
            (&self.credential_security, replaced)
            && credential_security
                .close_password_session(&replaced)
                .is_err()
        {
            if identity.connection_owned {
                let _ = credential_security.close_password_session(&identity.credential_session_id);
            }
            return Err(Self::access_error(PublicError::PolicyInvariant));
        }

        debug!("LOGON completed");
        Ok(vec![BoltResponse::Success(SuccessMessage::new())])
    }

    /// Handle LOGOFF message (de-authentication).
    async fn handle_logoff(&mut self) -> BoltResult<Vec<BoltResponse>> {
        self.session.write().end_query();
        self.close_installed_session();

        debug!("LOGOFF completed");
        Ok(vec![BoltResponse::Success(SuccessMessage::new())])
    }

    /// Send a FAILURE response for an error.
    async fn send_failure<T: AsyncRead + AsyncWrite + Unpin>(
        &mut self,
        framed: &mut Framed<T, BoltRequestCodec>,
        error: &BoltError,
    ) -> BoltResult<()> {
        let failure = error.to_failure();

        // Transition to FAILED state
        {
            let mut session = self.session.write();
            session.state_machine_mut().set_error(&failure.message);
            let _ = session.state_machine_mut().transition(BoltEvent::Failure);
        }

        tokio::time::timeout(
            self.config.connection_timeout,
            framed.send(BoltResponse::Failure(failure)),
        )
        .await
        .map_err(|_| BoltError::Timeout)??;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures::{SinkExt, StreamExt};
    use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
    use tokio::io::DuplexStream;
    use tokio_util::codec::Framed;
    use zeta4j_credential::{KeyVersion, SessionStatus};

    use super::*;
    use crate::server::auth::SignerReference;
    use crate::server::bolt::codec::BoltResponseCodec;
    use crate::server::system::{
        CredentialRuntimePolicy, CredentialStore, LocalCredentialStore, SystemStorage,
        initialize_local_credential_runtime,
    };

    const USERNAME: &str = "bolt-user";
    const PASSWORD: &str = "BoltPassword!234";

    struct Fixture {
        _temp: tempfile::TempDir,
        store: LocalCredentialStore,
        client: Framed<DuplexStream, BoltResponseCodec>,
    }

    type Connection = BoltConnection<DuplexStream>;

    impl Fixture {
        fn new(idle_timeout: Duration) -> Result<(Self, Connection), Box<dyn std::error::Error>> {
            let temp = crate::server::system::owner_only_tempdir()?;
            let storage = Arc::new(SystemStorage::open(temp.path().join("system"))?);
            let security = Arc::new(SecurityManager::new(storage.clone()));
            security.create_user(USERNAME, PASSWORD)?;
            let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                .map_err(|_| std::io::Error::other("failed to generate signer"))?;
            let signer_path = temp.path().join("bolt-signer.pk8");
            std::fs::write(&signer_path, document.as_ref())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&signer_path, std::fs::Permissions::from_mode(0o600))?;
            }
            let reference = SignerReference::new(signer_path, "bolt-key-1", KeyVersion::new(1)?)?;
            let (credential_security, credential_tokens) = initialize_local_credential_runtime(
                security,
                &reference,
                CredentialRuntimePolicy::default(),
                "base",
            )?;

            let session = Arc::new(RwLock::new(BoltSession::new()));
            {
                let mut session = session.write();
                session
                    .state_machine_mut()
                    .transition(BoltEvent::HandshakeReceived)?;
                session
                    .state_machine_mut()
                    .transition(BoltEvent::VersionNegotiated(BoltVersion::V5_0))?;
                session.set_version(BoltVersion::V5_0);
            }
            let config = Arc::new(BoltConfig {
                connection_timeout: idle_timeout,
                ..BoltConfig::default()
            });
            let (client, server) = tokio::io::duplex(64 * 1024);
            let connection = BoltConnection::new(server, config, session)
                .with_credential_security(credential_security)
                .with_credential_tokens(credential_tokens)
                .with_client_address("127.0.0.1");
            let fixture = Self {
                _temp: temp,
                store: LocalCredentialStore::new(storage),
                client: Framed::new(client, BoltResponseCodec::new()),
            };
            Ok((fixture, connection))
        }

        fn active_sessions(&self) -> Result<usize, Box<dyn std::error::Error>> {
            Ok(self
                .store
                .sessions()?
                .into_iter()
                .filter(|session| session.status == SessionStatus::Active)
                .count())
        }
    }

    async fn hello(
        client: &mut Framed<DuplexStream, BoltResponseCodec>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        client
            .send(BoltRequest::Hello(
                HelloMessage::new("test/1.0").with_auth(AuthToken::basic(USERNAME, PASSWORD)),
            ))
            .await?;
        match client.next().await {
            Some(Ok(BoltResponse::Success(_))) => Ok(()),
            other => Err(std::io::Error::other(format!("HELLO failed: {other:?}")).into()),
        }
    }

    #[tokio::test]
    async fn idle_timeout_closes_the_durable_session() -> Result<(), Box<dyn std::error::Error>> {
        let (mut fixture, mut connection) = Fixture::new(Duration::from_millis(200))?;
        let running = tokio::spawn(async move { connection.run().await });
        hello(&mut fixture.client).await?;
        assert_eq!(fixture.active_sessions()?, 1);

        // The client stays connected but silent past the idle timeout.
        let outcome = running.await?;
        assert!(matches!(outcome, Err(BoltError::Timeout)));
        assert_eq!(fixture.active_sessions()?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_connection_closes_the_durable_session()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut fixture, mut connection) = Fixture::new(Duration::from_secs(60))?;
        let running = tokio::spawn(async move { connection.run().await });
        hello(&mut fixture.client).await?;
        assert_eq!(fixture.active_sessions()?, 1);

        running.abort();
        assert!(running.await.is_err_and(|error| error.is_cancelled()));
        assert_eq!(fixture.active_sessions()?, 0);
        Ok(())
    }
}
