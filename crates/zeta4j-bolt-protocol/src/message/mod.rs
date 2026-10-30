//! Bolt protocol message types shared by server and clients.
//!
//! This module implements all Bolt protocol request and response messages
//! for versions 4.0 through 5.0.

pub mod metadata;
pub mod request;
pub mod response;

pub use metadata::*;
pub use request::*;
pub use response::*;

/// Bolt message tags for request messages.
pub mod tag {
    /// HELLO message tag (0x01)
    pub const HELLO: u8 = 0x01;
    /// GOODBYE message tag (0x02)
    pub const GOODBYE: u8 = 0x02;
    /// RESET message tag (0x0F)
    pub const RESET: u8 = 0x0F;
    /// RUN message tag (0x10)
    pub const RUN: u8 = 0x10;
    /// BEGIN message tag (0x11)
    pub const BEGIN: u8 = 0x11;
    /// COMMIT message tag (0x12)
    pub const COMMIT: u8 = 0x12;
    /// ROLLBACK message tag (0x13)
    pub const ROLLBACK: u8 = 0x13;
    /// DISCARD message tag (0x2F)
    pub const DISCARD: u8 = 0x2F;
    /// PULL message tag (0x3F)
    pub const PULL: u8 = 0x3F;
    /// ROUTE message tag (0x66) - Bolt 4.3+
    pub const ROUTE: u8 = 0x66;
    /// LOGON message tag (0x6A) - Bolt 5.1+
    pub const LOGON: u8 = 0x6A;
    /// LOGOFF message tag (0x6B) - Bolt 5.1+
    pub const LOGOFF: u8 = 0x6B;
    /// TELEMETRY message tag (0x54) - Bolt 5.4+
    pub const TELEMETRY: u8 = 0x54;

    /// SUCCESS response tag (0x70)
    pub const SUCCESS: u8 = 0x70;
    /// RECORD response tag (0x71)
    pub const RECORD: u8 = 0x71;
    /// IGNORED response tag (0x7E)
    pub const IGNORED: u8 = 0x7E;
    /// FAILURE response tag (0x7F)
    pub const FAILURE: u8 = 0x7F;
}
