//! Bolt protocol handshake implementation.
//!
//! The Bolt handshake consists of:
//! 1. Client sends 4-byte magic number (0x6060B017)
//! 2. Client sends 4 x 4-byte version proposals (highest first)
//! 3. Server responds with 4-byte agreed version (or 0 if none)

mod negotiation;

pub use negotiation::{Handshake, HandshakeResult};
pub use zeta4j_bolt_protocol::{BOLT_MAGIC, BoltVersion};

// Re-export error from parent module
pub use super::error::HandshakeError;

/// Bolt protocol magic number: 0x6060B017
/// This identifies a Bolt connection.
/// Size of the complete handshake message from client (magic + 4 versions)
pub const HANDSHAKE_SIZE: usize = 20;

/// Size of server response (negotiated version)
pub const HANDSHAKE_RESPONSE_SIZE: usize = 4;
