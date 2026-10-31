// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bolt Protocol implementation.
//!
//! This module implements the Bolt protocol versions 4.0 through 5.0,
//! enabling compatibility with Bolt-compatible drivers.

pub mod codec;
pub mod config;
pub mod connection;
pub mod error;
pub mod executor;
pub mod handshake;
pub mod message;
pub mod packstream;
pub mod server;
pub mod state;

pub use codec::{BoltCodec, BoltRequestCodec};
pub use config::{BoltConfig, BoltConfigBuilder};
pub use connection::handler::AsyncQueryExecutor;
pub use connection::{BoltConnection, BoltSession, SessionId};
pub use error::{BoltError, BoltErrorCode, BoltResult};
pub use executor::BoltQueryExecutor;
pub use handshake::{BoltVersion, Handshake, HandshakeError};
pub use message::{
    AccessMode, AuthToken, BeginMessage, BoltRequest, BoltResponse, DiscardMessage, FailureMessage,
    HelloMessage, LogonMessage, Notification, NotificationSeverity, PullMessage, QueryPlan,
    QueryStats, RecordMessage, RouteMessage, RoutingTable, RunMessage, SuccessMessage,
};
pub use packstream::{
    PackStreamDecoder, PackStreamEncoder, PackStreamError, PackStreamNode, PackStreamRelationship,
    PackStreamStructure, PackStreamValue,
};
pub use server::{
    BoltServer, BoltServerBuilder, BoltServerState, BoltServerStats, BoltServerStatsSnapshot,
};
pub use state::{BoltEvent, BoltState, BoltStateMachine};
