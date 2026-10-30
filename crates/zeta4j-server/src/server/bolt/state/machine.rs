//! Bolt connection state machine.
//!
//! Implements the Bolt protocol state machine with proper
//! state transitions and message validation.

use crate::server::bolt::BoltError;
use crate::server::bolt::handshake::BoltVersion;
use crate::server::bolt::message::{BoltRequest, BoltResponse};

/// Bolt connection states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoltState {
    /// Initial state, waiting for handshake
    Disconnected,
    /// Handshake received, negotiating version
    Negotiation,
    /// HELLO received, authenticating
    Authentication,
    /// Ready to accept queries
    Ready,
    /// Currently streaming results from a query
    Streaming,
    /// Inside a transaction, ready for queries
    TxReady,
    /// Inside a transaction, streaming results
    TxStreaming,
    /// Connection has failed, waiting for RESET
    Failed,
    /// Connection is unusable (protocol error)
    Defunct,
}

impl BoltState {
    /// Get state name for logging.
    pub fn name(&self) -> &'static str {
        match self {
            BoltState::Disconnected => "DISCONNECTED",
            BoltState::Negotiation => "NEGOTIATION",
            BoltState::Authentication => "AUTHENTICATION",
            BoltState::Ready => "READY",
            BoltState::Streaming => "STREAMING",
            BoltState::TxReady => "TX_READY",
            BoltState::TxStreaming => "TX_STREAMING",
            BoltState::Failed => "FAILED",
            BoltState::Defunct => "DEFUNCT",
        }
    }

    /// Check if the state allows queries.
    pub fn can_run_queries(&self) -> bool {
        matches!(self, BoltState::Ready | BoltState::TxReady)
    }

    /// Check if the state is inside a transaction.
    pub fn in_transaction(&self) -> bool {
        matches!(self, BoltState::TxReady | BoltState::TxStreaming)
    }

    /// Check if the state is streaming results.
    pub fn is_streaming(&self) -> bool {
        matches!(self, BoltState::Streaming | BoltState::TxStreaming)
    }

    /// Check if the connection is usable.
    pub fn is_usable(&self) -> bool {
        !matches!(self, BoltState::Defunct | BoltState::Disconnected)
    }
}

/// Events that trigger state transitions.
#[derive(Debug, Clone)]
pub enum BoltEvent {
    /// Handshake data received
    HandshakeReceived,
    /// Version negotiation succeeded
    VersionNegotiated(BoltVersion),
    /// Version negotiation failed (no common version)
    VersionRejected,
    /// HELLO succeeded (authenticated)
    HelloSuccess,
    /// HELLO failed (auth error)
    HelloFailure,
    /// RUN succeeded (query started)
    RunSuccess,
    /// PULL completed (all results consumed)
    PullComplete,
    /// PULL has more results
    PullMore,
    /// DISCARD completed
    DiscardComplete,
    /// BEGIN succeeded (transaction started)
    BeginSuccess,
    /// COMMIT succeeded (transaction committed)
    CommitSuccess,
    /// ROLLBACK succeeded (transaction rolled back)
    RollbackSuccess,
    /// Operation failed
    Failure,
    /// RESET received
    Reset,
    /// GOODBYE received
    Goodbye,
    /// Connection closed
    ConnectionClosed,
    /// Protocol error (unrecoverable)
    ProtocolError,
}

/// Bolt protocol state machine.
#[derive(Debug)]
pub struct BoltStateMachine {
    /// Current state
    state: BoltState,
    /// Negotiated protocol version
    version: Option<BoltVersion>,
    /// Last error message
    last_error: Option<String>,
    /// Pending query ID
    pending_qid: Option<i64>,
}

impl BoltStateMachine {
    /// Create a new state machine in disconnected state.
    pub fn new() -> Self {
        Self {
            state: BoltState::Disconnected,
            version: None,
            last_error: None,
            pending_qid: None,
        }
    }

    /// Get current state.
    pub fn state(&self) -> BoltState {
        self.state
    }

    /// Get negotiated version.
    pub fn version(&self) -> Option<BoltVersion> {
        self.version
    }

    /// Get last error.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Get pending query ID.
    pub fn pending_qid(&self) -> Option<i64> {
        self.pending_qid
    }

    /// Set pending query ID.
    pub fn set_pending_qid(&mut self, qid: Option<i64>) {
        self.pending_qid = qid;
    }

    /// Process an event and transition to new state.
    pub fn transition(&mut self, event: BoltEvent) -> Result<BoltState, BoltError> {
        let new_state = self.next_state(&event)?;

        // Handle version storage
        if let BoltEvent::VersionNegotiated(version) = &event {
            self.version = Some(*version);
        }

        // Clear error on successful reset
        if matches!(event, BoltEvent::Reset) {
            self.last_error = None;
        }

        self.state = new_state;
        Ok(new_state)
    }

    /// Calculate the next state for an event.
    fn next_state(&self, event: &BoltEvent) -> Result<BoltState, BoltError> {
        match (self.state, event) {
            // From Disconnected
            (BoltState::Disconnected, BoltEvent::HandshakeReceived) => Ok(BoltState::Negotiation),
            (BoltState::Disconnected, BoltEvent::ConnectionClosed) => Ok(BoltState::Disconnected),

            // From Negotiation
            (BoltState::Negotiation, BoltEvent::VersionNegotiated(_)) => {
                Ok(BoltState::Authentication)
            }
            (BoltState::Negotiation, BoltEvent::VersionRejected) => Ok(BoltState::Defunct),

            // From Authentication
            (BoltState::Authentication, BoltEvent::HelloSuccess) => Ok(BoltState::Ready),
            (BoltState::Authentication, BoltEvent::HelloFailure) => Ok(BoltState::Defunct),

            // From Ready
            (BoltState::Ready, BoltEvent::RunSuccess) => Ok(BoltState::Streaming),
            (BoltState::Ready, BoltEvent::BeginSuccess) => Ok(BoltState::TxReady),
            (BoltState::Ready, BoltEvent::Failure) => Ok(BoltState::Failed),
            (BoltState::Ready, BoltEvent::Goodbye) => Ok(BoltState::Disconnected),
            (BoltState::Ready, BoltEvent::Reset) => Ok(BoltState::Ready),

            // From Streaming
            (BoltState::Streaming, BoltEvent::PullComplete) => Ok(BoltState::Ready),
            (BoltState::Streaming, BoltEvent::PullMore) => Ok(BoltState::Streaming),
            (BoltState::Streaming, BoltEvent::DiscardComplete) => Ok(BoltState::Ready),
            (BoltState::Streaming, BoltEvent::Failure) => Ok(BoltState::Failed),
            (BoltState::Streaming, BoltEvent::Reset) => Ok(BoltState::Ready),

            // From TxReady
            (BoltState::TxReady, BoltEvent::RunSuccess) => Ok(BoltState::TxStreaming),
            (BoltState::TxReady, BoltEvent::CommitSuccess) => Ok(BoltState::Ready),
            (BoltState::TxReady, BoltEvent::RollbackSuccess) => Ok(BoltState::Ready),
            (BoltState::TxReady, BoltEvent::Failure) => Ok(BoltState::Failed),
            (BoltState::TxReady, BoltEvent::Reset) => Ok(BoltState::Ready),

            // From TxStreaming
            (BoltState::TxStreaming, BoltEvent::PullComplete) => Ok(BoltState::TxReady),
            (BoltState::TxStreaming, BoltEvent::PullMore) => Ok(BoltState::TxStreaming),
            (BoltState::TxStreaming, BoltEvent::DiscardComplete) => Ok(BoltState::TxReady),
            (BoltState::TxStreaming, BoltEvent::Failure) => Ok(BoltState::Failed),
            (BoltState::TxStreaming, BoltEvent::Reset) => Ok(BoltState::Ready),

            // From Failed (can only reset or close)
            (BoltState::Failed, BoltEvent::Reset) => Ok(BoltState::Ready),
            (BoltState::Failed, BoltEvent::ConnectionClosed) => Ok(BoltState::Disconnected),
            (BoltState::Failed, BoltEvent::Goodbye) => Ok(BoltState::Disconnected),

            // From Defunct (can only close)
            (BoltState::Defunct, BoltEvent::ConnectionClosed) => Ok(BoltState::Disconnected),

            // Protocol error from any state
            (_, BoltEvent::ProtocolError) => Ok(BoltState::Defunct),

            // Connection closed from any state
            (_, BoltEvent::ConnectionClosed) => Ok(BoltState::Disconnected),

            // Invalid transition
            (state, event) => Err(BoltError::InvalidState(format!(
                "Cannot process {:?} in {} state",
                event,
                state.name()
            ))),
        }
    }

    /// Set error message when entering failed state.
    pub fn set_error(&mut self, error: &str) {
        self.last_error = Some(error.to_string());
    }

    /// Check if a request message can be processed in current state.
    pub fn can_accept(&self, message: &BoltRequest) -> bool {
        match message {
            BoltRequest::Hello(_) => self.state == BoltState::Authentication,
            BoltRequest::Goodbye => {
                self.state != BoltState::Disconnected && self.state != BoltState::Defunct
            }
            BoltRequest::Reset => self.state != BoltState::Disconnected,
            BoltRequest::Run(_) => matches!(self.state, BoltState::Ready | BoltState::TxReady),
            BoltRequest::Pull(_) | BoltRequest::Discard(_) => {
                matches!(self.state, BoltState::Streaming | BoltState::TxStreaming)
            }
            BoltRequest::Begin(_) => self.state == BoltState::Ready,
            BoltRequest::Commit | BoltRequest::Rollback => self.state == BoltState::TxReady,
            BoltRequest::Route(_) => matches!(self.state, BoltState::Ready | BoltState::TxReady),
            BoltRequest::Logon(_) => self.state == BoltState::Ready,
            BoltRequest::Logoff => self.state == BoltState::Ready,
        }
    }

    /// Get the expected response type for a request.
    pub fn expected_response(&self, request: &BoltRequest) -> ExpectedResponse {
        match request {
            BoltRequest::Hello(_) => ExpectedResponse::Success,
            BoltRequest::Goodbye => ExpectedResponse::None,
            BoltRequest::Reset => ExpectedResponse::Success,
            BoltRequest::Run(_) => ExpectedResponse::Success,
            BoltRequest::Pull(_) => ExpectedResponse::RecordsThenSuccess,
            BoltRequest::Discard(_) => ExpectedResponse::Success,
            BoltRequest::Begin(_) => ExpectedResponse::Success,
            BoltRequest::Commit => ExpectedResponse::Success,
            BoltRequest::Rollback => ExpectedResponse::Success,
            BoltRequest::Route(_) => ExpectedResponse::Success,
            BoltRequest::Logon(_) => ExpectedResponse::Success,
            BoltRequest::Logoff => ExpectedResponse::Success,
        }
    }

    /// Process a response and update state accordingly.
    pub fn process_response(
        &mut self,
        request: &BoltRequest,
        response: &BoltResponse,
    ) -> Result<(), BoltError> {
        let event = match response {
            BoltResponse::Success(msg) => {
                match request {
                    BoltRequest::Hello(_) => BoltEvent::HelloSuccess,
                    BoltRequest::Run(_) => BoltEvent::RunSuccess,
                    BoltRequest::Pull(_) => {
                        if msg.has_more() {
                            BoltEvent::PullMore
                        } else {
                            BoltEvent::PullComplete
                        }
                    }
                    BoltRequest::Discard(_) => BoltEvent::DiscardComplete,
                    BoltRequest::Begin(_) => BoltEvent::BeginSuccess,
                    BoltRequest::Commit => BoltEvent::CommitSuccess,
                    BoltRequest::Rollback => BoltEvent::RollbackSuccess,
                    BoltRequest::Reset => BoltEvent::Reset,
                    _ => return Ok(()), // No state change for other messages
                }
            }
            BoltResponse::Failure(msg) => {
                self.set_error(&msg.message);
                if matches!(request, BoltRequest::Hello(_)) {
                    BoltEvent::HelloFailure
                } else {
                    BoltEvent::Failure
                }
            }
            BoltResponse::Ignored => {
                // No state change for ignored messages
                return Ok(());
            }
            BoltResponse::Record(_) => {
                // Records don't change state
                return Ok(());
            }
        };

        self.transition(event)?;
        Ok(())
    }
}

impl Default for BoltStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Expected response type for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedResponse {
    /// Expect SUCCESS or FAILURE
    Success,
    /// Expect zero or more RECORD followed by SUCCESS or FAILURE
    RecordsThenSuccess,
    /// No response expected (GOODBYE)
    None,
}
