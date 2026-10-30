//! Bolt protocol version definitions shared by server and clients.

use std::fmt;

/// Bolt protocol versions.
///
/// Version numbers are encoded as 4-byte big-endian integers:
/// - Major version in high 2 bytes
/// - Minor version in low 2 bytes
///
/// For example: V4_3 = 0x0004_0003 (major=4, minor=3)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum BoltVersion {
    /// Bolt 4.0 (Neo4j 4.0)
    V4_0 = 0x0004_0000,
    /// Bolt 4.1 (Neo4j 4.1)
    V4_1 = 0x0004_0001,
    /// Bolt 4.2 (Neo4j 4.2)
    V4_2 = 0x0004_0002,
    /// Bolt 4.3 (Neo4j 4.3) - Added ROUTE message
    V4_3 = 0x0004_0003,
    /// Bolt 4.4 (Neo4j 4.4)
    V4_4 = 0x0004_0004,
    /// Bolt 5.0 (Neo4j 5.0) - Element IDs, LOGON/LOGOFF
    V5_0 = 0x0005_0000,
}

impl BoltVersion {
    /// All supported versions in order of preference (newest first).
    pub const ALL: [BoltVersion; 6] = [
        BoltVersion::V5_0,
        BoltVersion::V4_4,
        BoltVersion::V4_3,
        BoltVersion::V4_2,
        BoltVersion::V4_1,
        BoltVersion::V4_0,
    ];

    /// Create a BoltVersion from a raw u32 value.
    ///
    /// Supports both client proposal format [0, range, minor, major] (0x00RRmmMM)
    /// and internal format (0x00MM00mm).
    pub fn from_u32(value: u32) -> Option<Self> {
        // Try internal format first (backwards compatibility)
        match value {
            0x0004_0000 => return Some(BoltVersion::V4_0),
            0x0004_0001 => return Some(BoltVersion::V4_1),
            0x0004_0002 => return Some(BoltVersion::V4_2),
            0x0004_0003 => return Some(BoltVersion::V4_3),
            0x0004_0004 => return Some(BoltVersion::V4_4),
            0x0005_0000 => return Some(BoltVersion::V5_0),
            _ => {}
        }

        // Try client proposal format: [0, range, minor, major] = 0x00_RR_mm_MM
        let major = (value & 0xFF) as u16;
        let minor = ((value >> 8) & 0xFF) as u16;

        if major == 0 {
            return None;
        }

        // Convert to internal format and match
        let internal = ((major as u32) << 16) | (minor as u32);
        match internal {
            0x0004_0000 => Some(BoltVersion::V4_0),
            0x0004_0001 => Some(BoltVersion::V4_1),
            0x0004_0002 => Some(BoltVersion::V4_2),
            0x0004_0003 => Some(BoltVersion::V4_3),
            0x0004_0004 => Some(BoltVersion::V4_4),
            0x0005_0000 => Some(BoltVersion::V5_0),
            _ => None,
        }
    }

    /// Get the raw u32 value.
    pub fn as_u32(self) -> u32 {
        self as u32
    }

    /// Get the major version number.
    pub fn major(self) -> u16 {
        ((self as u32) >> 16) as u16
    }

    /// Get the minor version number.
    pub fn minor(self) -> u16 {
        ((self as u32) & 0xFFFF) as u16
    }

    /// Convert to Bolt protocol response format.
    ///
    /// Response format: [0, 0, minor, major]
    /// This matches Neo4j's server response format.
    pub fn to_bytes(self) -> [u8; 4] {
        [0x00, 0x00, self.minor() as u8, self.major() as u8]
    }

    /// Parse from big-endian bytes.
    pub fn from_bytes(bytes: [u8; 4]) -> Option<Self> {
        let value = u32::from_be_bytes(bytes);
        Self::from_u32(value)
    }

    /// Check if this version supports the ROUTE message.
    pub fn supports_route(self) -> bool {
        matches!(
            self,
            BoltVersion::V4_3 | BoltVersion::V4_4 | BoltVersion::V5_0
        )
    }

    /// Check if this version uses element IDs (Neo4j 5.x style).
    pub fn uses_element_ids(self) -> bool {
        matches!(self, BoltVersion::V5_0)
    }

    /// Check if this version supports LOGON/LOGOFF messages.
    pub fn supports_logon(self) -> bool {
        matches!(self, BoltVersion::V5_0)
    }
}

impl fmt::Display for BoltVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major(), self.minor())
    }
}

impl PartialOrd for BoltVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BoltVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_u32().cmp(&other.as_u32())
    }
}
