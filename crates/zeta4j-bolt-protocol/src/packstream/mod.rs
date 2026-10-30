//! Shared PackStream serialization format.
//!
//! PackStream is the binary serialization format used by the Bolt protocol
//! to encode values for transmission between client and server.
//!
//! # Supported Types
//!
//! - **Null**: Single byte marker
//! - **Boolean**: True/False markers
//! - **Integer**: Variable-length encoding (-2^63 to 2^63-1)
//! - **Float**: 64-bit IEEE 754
//! - **String**: UTF-8 encoded, variable length prefix
//! - **Bytes**: Raw bytes, variable length prefix
//! - **List**: Homogeneous or heterogeneous collections
//! - **Map**: String keys to arbitrary values
//! - **Structure**: Tagged structures for graph types
//!
//! # Graph Structures
//!
//! - **Node**: id, labels, properties
//! - **Relationship**: id, start_id, end_id, type, properties
//! - **Path**: nodes, relationships, indices
//!
//! # Temporal Structures
//!
//! - **Date**: Days since Unix epoch
//! - **Time**: Nanoseconds since midnight + timezone
//! - **LocalTime**: Nanoseconds since midnight (no timezone)
//! - **DateTime**: Seconds + nanoseconds + timezone
//! - **LocalDateTime**: Seconds + nanoseconds (no timezone)
//! - **Duration**: Months, days, seconds, nanoseconds
//!
//! # Spatial Structures
//!
//! - **Point2D**: SRID + x, y coordinates
//! - **Point3D**: SRID + x, y, z coordinates

pub mod decoder;
pub mod encoder;
pub mod marker;
pub mod structures;
pub mod types;

pub use decoder::{PackStreamDecoder, decode};
pub use encoder::{PackStreamEncoder, encode};
pub use marker::*;
pub use structures::{
    PackStreamDate, PackStreamDateTime, PackStreamDuration, PackStreamLocalDateTime,
    PackStreamLocalTime, PackStreamNode, PackStreamPath, PackStreamPoint2D, PackStreamPoint3D,
    PackStreamRelationship, PackStreamTime, PackStreamUnboundRelationship, PackStreamVector,
};
pub use types::{PackStreamStructure, PackStreamValue};

use std::fmt;

/// PackStream errors.
#[derive(Debug, Clone)]
pub enum PackStreamError {
    /// Unexpected end of input
    UnexpectedEof,
    /// Unknown marker byte
    UnknownMarker(u8),
    /// Invalid UTF-8 in string
    InvalidUtf8(String),
    /// Invalid map key (must be string)
    InvalidMapKey,
    /// Value too large to encode
    ValueTooLarge(&'static str, usize),
    /// Invalid structure format
    InvalidStructure(String),
    /// Recursive container nesting exceeds the decoder safety boundary.
    NestingTooDeep { max: usize },
}

impl fmt::Display for PackStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackStreamError::UnexpectedEof => write!(f, "Unexpected end of PackStream data"),
            PackStreamError::UnknownMarker(m) => {
                write!(f, "Unknown PackStream marker: 0x{:02X}", m)
            }
            PackStreamError::InvalidUtf8(e) => write!(f, "Invalid UTF-8 in string: {}", e),
            PackStreamError::InvalidMapKey => write!(f, "Map keys must be strings"),
            PackStreamError::ValueTooLarge(t, s) => write!(f, "{} too large: {} bytes", t, s),
            PackStreamError::InvalidStructure(msg) => write!(f, "Invalid structure: {}", msg),
            PackStreamError::NestingTooDeep { max } => {
                write!(f, "PackStream nesting exceeds maximum depth {max}")
            }
        }
    }
}

impl std::error::Error for PackStreamError {}
