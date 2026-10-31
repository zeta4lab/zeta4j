// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bolt connection handling.
//!
//! This module manages Bolt protocol connections including
//! handshake, authentication, and message processing.

pub mod handler;
pub mod session;

pub use handler::BoltConnection;
pub use session::{BoltSession, SessionId};
