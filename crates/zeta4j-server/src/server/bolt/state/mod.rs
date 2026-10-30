//! Bolt connection state management.
//!
//! This module implements the Bolt protocol state machine that
//! manages connection lifecycle and message flow.

pub mod machine;

pub use machine::{BoltEvent, BoltState, BoltStateMachine};
