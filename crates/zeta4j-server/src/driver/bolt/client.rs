// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! High-level Bolt client for driver use.
//!
//! Provides a convenient API for executing queries and transactions.

use std::collections::HashMap;

use crate::server::bolt::message::AuthToken as BoltAuthToken;
use crate::server::bolt::packstream::PackStreamValue;
use crate::server::bolt::{
    BeginMessage, BoltError, BoltRequest, BoltResponse, BoltResult, BoltVersion, HelloMessage,
    PullMessage, RouteMessage, RunMessage,
};

use super::super::driver::ServerAddress;
use super::CLIENT_USER_AGENT;
use super::connection::BoltConnection;

/// Bolt client state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoltClientState {
    /// Not authenticated
    Disconnected,
    /// Connected and authenticated
    Ready,
    /// In transaction
    InTransaction,
    /// Transaction failed (needs reset)
    Failed,
    /// Streaming results
    Streaming,
}

/// Query result from a RUN + PULL sequence
#[derive(Debug, Default)]
pub struct BoltQueryResult {
    /// Column names (keys)
    pub keys: Vec<String>,
    /// Result records
    pub records: Vec<Vec<PackStreamValue>>,
    /// Query statistics
    pub stats: Option<HashMap<String, PackStreamValue>>,
    /// Bookmark (for causal consistency)
    pub bookmark: Option<String>,
    /// Server info
    pub server: Option<String>,
    /// Database name
    pub database: Option<String>,
    /// Has more results?
    pub has_more: bool,
}

/// Routing information from ROUTE response
#[derive(Debug, Clone)]
pub struct RoutingInfo {
    /// Time-to-live in seconds
    pub ttl: u64,
    /// Router addresses
    pub routers: Vec<ServerAddress>,
    /// Writer addresses
    pub writers: Vec<ServerAddress>,
    /// Reader addresses
    pub readers: Vec<ServerAddress>,
    /// Database name
    pub database: Option<String>,
}

impl RoutingInfo {
    /// Parse routing info from SUCCESS response
    pub fn from_success(metadata: &HashMap<String, PackStreamValue>) -> Option<Self> {
        // ROUTE response has "rt" field containing routing table
        let rt = metadata.get("rt")?.as_map()?;

        // Parse TTL
        let ttl = rt
            .get("ttl")
            .and_then(|v| v.as_int())
            .map(|t| t as u64)
            .unwrap_or(300);

        // Parse servers array
        let servers = rt.get("servers")?;
        let servers_list = if let PackStreamValue::List(list) = servers {
            list
        } else {
            return None;
        };

        let mut routers = Vec::new();
        let mut writers = Vec::new();
        let mut readers = Vec::new();

        for server in servers_list {
            if let PackStreamValue::Map(server_map) = server {
                // Get role
                let role = server_map
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Get addresses and parse them
                let parsed_addrs: Vec<ServerAddress> = server_map
                    .get("addresses")
                    .and_then(|v| {
                        if let PackStreamValue::List(list) = v {
                            Some(list)
                        } else {
                            None
                        }
                    })
                    .map(|addresses| {
                        addresses
                            .iter()
                            .filter_map(|a| a.as_str().and_then(parse_server_address))
                            .collect()
                    })
                    .unwrap_or_default();

                match role {
                    "ROUTE" => routers.extend(parsed_addrs),
                    "WRITE" => writers.extend(parsed_addrs),
                    "READ" => readers.extend(parsed_addrs),
                    _ => {}
                }
            }
        }

        // Parse database name
        let database = rt.get("db").and_then(|v| v.as_str()).map(|s| s.to_string());

        Some(Self {
            ttl,
            routers,
            writers,
            readers,
            database,
        })
    }
}

impl Default for RoutingInfo {
    fn default() -> Self {
        Self {
            ttl: 300,
            routers: Vec::new(),
            writers: Vec::new(),
            readers: Vec::new(),
            database: None,
        }
    }
}

/// Parse server address from string (e.g., "host:port")
fn parse_server_address(s: &str) -> Option<ServerAddress> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        1 => Some(ServerAddress::new(parts[0], 9045)),
        2 => {
            let port = parts[1].parse().ok()?;
            Some(ServerAddress::new(parts[0], port))
        }
        _ => None,
    }
}

/// High-level Bolt client.
///
/// Wraps a BoltConnection and provides a convenient API for:
/// - Authentication (HELLO)
/// - Query execution (RUN + PULL)
/// - Transaction management (BEGIN, COMMIT, ROLLBACK)
pub struct BoltClient {
    /// Underlying connection
    connection: BoltConnection,
    /// Client state
    state: BoltClientState,
    /// Server agent
    server_agent: Option<String>,
    /// Connection ID
    connection_id: Option<String>,
    /// Current database
    database: Option<String>,
}

impl BoltClient {
    /// Create a new Bolt client and connect to the server.
    ///
    /// This performs TCP connection and handshake, but not authentication.
    pub async fn connect(address: &str) -> BoltResult<Self> {
        let mut connection = BoltConnection::connect(address).await?;
        connection.handshake().await?;

        Ok(Self {
            connection,
            state: BoltClientState::Disconnected,
            server_agent: None,
            connection_id: None,
            database: None,
        })
    }

    /// Authenticate with the server using HELLO.
    pub async fn hello(&mut self, auth: Option<BoltAuthToken>) -> BoltResult<()> {
        let auth_token = auth.unwrap_or_else(BoltAuthToken::none);
        let hello = HelloMessage::new(CLIENT_USER_AGENT).with_auth(auth_token);
        let request = BoltRequest::Hello(hello);

        let response = self.connection.request(request).await?;

        match response {
            BoltResponse::Success(success) => {
                self.server_agent = success.server().map(String::from);
                self.connection_id = success.connection_id().map(String::from);
                self.state = BoltClientState::Ready;
                Ok(())
            }
            BoltResponse::Failure(failure) => {
                self.state = BoltClientState::Failed;
                Err(BoltError::Query(format!(
                    "{}: {}",
                    failure.code, failure.message
                )))
            }
            _ => Err(BoltError::Protocol(
                "Unexpected response to HELLO".to_string(),
            )),
        }
    }

    /// Execute a query and pull all results.
    ///
    /// This sends RUN followed by PULL(-1) to get all results.
    pub async fn run(
        &mut self,
        query: &str,
        parameters: HashMap<String, PackStreamValue>,
        database: Option<&str>,
    ) -> BoltResult<BoltQueryResult> {
        self.ensure_ready()?;

        // Remember if we're in a transaction to restore state after streaming
        let in_transaction = self.state == BoltClientState::InTransaction;

        // Build RUN message
        let mut run = RunMessage::new(query).with_parameters(parameters);
        if let Some(db) = database.or(self.database.as_deref()) {
            run = run.with_database(db);
        }

        // Send RUN
        let response = self.connection.request(BoltRequest::Run(run)).await?;
        let mut result = BoltQueryResult::default();

        match response {
            BoltResponse::Success(success) => {
                // Extract keys from SUCCESS
                if let Some(fields) = success.fields() {
                    result.keys = fields;
                }
            }
            BoltResponse::Failure(failure) => {
                self.state = BoltClientState::Failed;
                return Err(BoltError::Query(format!(
                    "{}: {}",
                    failure.code, failure.message
                )));
            }
            _ => {
                return Err(BoltError::Protocol(
                    "Unexpected response to RUN".to_string(),
                ));
            }
        }

        // Send PULL
        self.state = BoltClientState::Streaming;
        let pull = PullMessage::all();
        self.connection.send(BoltRequest::Pull(pull)).await?;

        // Collect records until SUCCESS or FAILURE
        loop {
            let response = self.connection.recv().await?;

            match response {
                BoltResponse::Record(record) => {
                    result.records.push(record.fields);
                }
                BoltResponse::Success(success) => {
                    result.has_more = success.has_more();
                    result.bookmark = success.bookmark().map(String::from);
                    result.database = success.db().map(String::from);
                    result.stats = success.stats().cloned();
                    // Restore state: InTransaction if we were in a tx, Ready otherwise
                    self.state = if in_transaction {
                        BoltClientState::InTransaction
                    } else {
                        BoltClientState::Ready
                    };
                    break;
                }
                BoltResponse::Failure(failure) => {
                    self.state = BoltClientState::Failed;
                    return Err(BoltError::Query(format!(
                        "{}: {}",
                        failure.code, failure.message
                    )));
                }
                _ => {
                    return Err(BoltError::Protocol(
                        "Unexpected response during PULL".to_string(),
                    ));
                }
            }
        }

        Ok(result)
    }

    /// Begin a transaction.
    pub async fn begin(
        &mut self,
        database: Option<&str>,
        bookmarks: Vec<String>,
    ) -> BoltResult<()> {
        self.ensure_ready()?;

        let mut begin = BeginMessage::new().with_bookmarks(bookmarks);
        if let Some(db) = database.or(self.database.as_deref()) {
            begin = begin.with_database(db);
        }

        let response = self.connection.request(BoltRequest::Begin(begin)).await?;

        match response {
            BoltResponse::Success(_) => {
                self.state = BoltClientState::InTransaction;
                Ok(())
            }
            BoltResponse::Failure(failure) => {
                self.state = BoltClientState::Failed;
                Err(BoltError::Query(format!(
                    "{}: {}",
                    failure.code, failure.message
                )))
            }
            _ => Err(BoltError::Protocol(
                "Unexpected response to BEGIN".to_string(),
            )),
        }
    }

    /// Commit the current transaction.
    pub async fn commit(&mut self) -> BoltResult<Option<String>> {
        if self.state != BoltClientState::InTransaction {
            return Err(BoltError::Protocol("Not in transaction".to_string()));
        }

        let response = self.connection.request(BoltRequest::Commit).await?;

        match response {
            BoltResponse::Success(success) => {
                self.state = BoltClientState::Ready;
                Ok(success.bookmark().map(String::from))
            }
            BoltResponse::Failure(failure) => {
                self.state = BoltClientState::Failed;
                Err(BoltError::Query(format!(
                    "{}: {}",
                    failure.code, failure.message
                )))
            }
            _ => Err(BoltError::Protocol(
                "Unexpected response to COMMIT".to_string(),
            )),
        }
    }

    /// Rollback the current transaction.
    pub async fn rollback(&mut self) -> BoltResult<()> {
        if self.state != BoltClientState::InTransaction {
            return Err(BoltError::Protocol("Not in transaction".to_string()));
        }

        let response = self.connection.request(BoltRequest::Rollback).await?;

        match response {
            BoltResponse::Success(_) => {
                self.state = BoltClientState::Ready;
                Ok(())
            }
            BoltResponse::Failure(failure) => {
                self.state = BoltClientState::Failed;
                Err(BoltError::Query(format!(
                    "{}: {}",
                    failure.code, failure.message
                )))
            }
            _ => Err(BoltError::Protocol(
                "Unexpected response to ROLLBACK".to_string(),
            )),
        }
    }

    /// Reset the connection state.
    ///
    /// Use this after a failure to restore the connection to READY state.
    pub async fn reset(&mut self) -> BoltResult<()> {
        let response = self.connection.request(BoltRequest::Reset).await?;

        match response {
            BoltResponse::Success(_) => {
                self.state = BoltClientState::Ready;
                Ok(())
            }
            BoltResponse::Failure(failure) => Err(BoltError::Query(format!(
                "{}: {}",
                failure.code, failure.message
            ))),
            _ => Err(BoltError::Protocol(
                "Unexpected response to RESET".to_string(),
            )),
        }
    }

    /// Get routing information from the server.
    ///
    /// Sends a ROUTE message and parses the routing table response.
    /// Available in Bolt 4.3+.
    pub async fn route(
        &mut self,
        database: Option<&str>,
        bookmarks: Vec<String>,
    ) -> BoltResult<RoutingInfo> {
        self.ensure_ready()?;

        // Build ROUTE message
        let mut route_msg = RouteMessage::new();
        if let Some(db) = database.or(self.database.as_deref()) {
            route_msg = route_msg.with_database(db);
        }
        route_msg.bookmarks = bookmarks;

        // Send ROUTE request
        let response = self
            .connection
            .request(BoltRequest::Route(route_msg))
            .await?;

        match response {
            BoltResponse::Success(success) => {
                // Parse routing info from SUCCESS metadata
                RoutingInfo::from_success(&success.metadata).ok_or_else(|| {
                    BoltError::Protocol("Invalid routing table in ROUTE response".to_string())
                })
            }
            BoltResponse::Failure(failure) => {
                self.state = BoltClientState::Failed;
                Err(BoltError::Query(format!(
                    "{}: {}",
                    failure.code, failure.message
                )))
            }
            _ => Err(BoltError::Protocol(
                "Unexpected response to ROUTE".to_string(),
            )),
        }
    }

    /// Close the connection gracefully.
    pub async fn close(&mut self) -> BoltResult<()> {
        self.state = BoltClientState::Disconnected;
        self.connection.close().await
    }

    /// Get the client state.
    pub fn state(&self) -> BoltClientState {
        self.state
    }

    /// Get the negotiated protocol version.
    pub fn protocol_version(&self) -> Option<BoltVersion> {
        self.connection.protocol_version()
    }

    /// Get the server agent string.
    pub fn server_agent(&self) -> Option<&str> {
        self.server_agent.as_deref()
    }

    /// Get the connection ID.
    pub fn connection_id(&self) -> Option<&str> {
        self.connection_id.as_deref()
    }

    /// Set the default database.
    pub fn set_database(&mut self, database: Option<String>) {
        self.database = database;
    }

    /// Get the default database.
    pub fn database(&self) -> Option<&str> {
        self.database.as_deref()
    }

    /// Check if client is ready for queries.
    pub fn is_ready(&self) -> bool {
        self.state == BoltClientState::Ready || self.state == BoltClientState::InTransaction
    }

    /// Ensure client is in ready state.
    fn ensure_ready(&self) -> BoltResult<()> {
        match self.state {
            BoltClientState::Ready | BoltClientState::InTransaction => Ok(()),
            BoltClientState::Disconnected => {
                Err(BoltError::Protocol("Not authenticated".to_string()))
            }
            BoltClientState::Failed => Err(BoltError::Protocol(
                "Connection failed, needs reset".to_string(),
            )),
            BoltClientState::Streaming => Err(BoltError::Protocol(
                "Currently streaming results".to_string(),
            )),
        }
    }
}

impl std::fmt::Debug for BoltClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoltClient")
            .field("state", &self.state)
            .field("protocol_version", &self.protocol_version())
            .field("server_agent", &self.server_agent)
            .field("connection_id", &self.connection_id)
            .field("database", &self.database)
            .finish()
    }
}
