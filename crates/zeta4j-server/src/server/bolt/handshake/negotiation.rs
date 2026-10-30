//! Bolt handshake negotiation.

use super::{BOLT_MAGIC, BoltVersion, HANDSHAKE_SIZE, HandshakeError};

/// Result of a successful handshake.
#[derive(Debug, Clone)]
pub struct HandshakeResult {
    /// Negotiated protocol version
    pub version: BoltVersion,
    /// Client's proposed versions (for debugging/logging)
    pub client_versions: [u32; 4],
}

/// Bolt handshake handler.
///
/// The handshake process:
/// 1. Client sends 20 bytes: 4-byte magic + 4 x 4-byte version proposals
/// 2. Server validates magic number
/// 3. Server finds highest mutually supported version
/// 4. Server responds with 4-byte agreed version (or 0x00000000 if none)
#[derive(Debug)]
pub struct Handshake {
    /// Supported versions on this server (ordered by preference, highest first)
    supported_versions: Vec<BoltVersion>,
}

impl Handshake {
    /// Create a new handshake handler with default supported versions.
    pub fn new() -> Self {
        Self {
            supported_versions: BoltVersion::ALL.to_vec(),
        }
    }

    /// Create a handshake handler with specific supported versions.
    pub fn with_versions(versions: Vec<BoltVersion>) -> Self {
        Self {
            supported_versions: versions,
        }
    }

    /// Process a client handshake and negotiate a version.
    ///
    /// # Arguments
    /// * `data` - The 20-byte handshake data from the client
    ///
    /// # Returns
    /// * `Ok(HandshakeResult)` - Successfully negotiated a version
    /// * `Err(HandshakeError)` - Handshake failed
    pub fn process(&self, data: &[u8]) -> Result<HandshakeResult, HandshakeError> {
        // Validate data length
        if data.len() < HANDSHAKE_SIZE {
            return Err(HandshakeError::InvalidData(format!(
                "Expected {} bytes, got {}",
                HANDSHAKE_SIZE,
                data.len()
            )));
        }

        // Validate magic number
        let mut magic = [0_u8; 4];
        magic.copy_from_slice(&data[..4]);
        if magic != BOLT_MAGIC {
            return Err(HandshakeError::InvalidMagic {
                expected: BOLT_MAGIC,
                received: magic,
            });
        }

        // Parse client version proposals
        let client_versions = self.parse_versions(&data[4..20]);

        // Find the highest mutually supported version
        let negotiated = self.negotiate_version(&client_versions)?;

        Ok(HandshakeResult {
            version: negotiated,
            client_versions,
        })
    }

    /// Parse 4 version proposals from bytes.
    fn parse_versions(&self, data: &[u8]) -> [u32; 4] {
        let mut versions = [0u32; 4];
        for (slot, chunk) in versions.iter_mut().zip(data.as_chunks::<4>().0) {
            *slot = u32::from_be_bytes(*chunk);
        }
        versions
    }

    /// Negotiate the best version from client proposals.
    fn negotiate_version(&self, client_versions: &[u32; 4]) -> Result<BoltVersion, HandshakeError> {
        // Client sends versions in order of preference (highest first)
        // We iterate through client's preferences and find first one we support
        for &client_version in client_versions {
            // Skip empty slots (0x00000000)
            if client_version == 0 {
                continue;
            }

            // Check if we support this version
            if let Some(version) = BoltVersion::from_u32(client_version)
                && self.supported_versions.contains(&version)
            {
                return Ok(version);
            }

            // Handle version ranges (Bolt 4.x allows minor version ranges)
            // Format: 0x00MM_mmRR where MM=major, mm=minor, RR=range
            // For example, 0x0004_0302 means 4.3 with range 2 (supports 4.3, 4.2, 4.1)
            if let Some(version) = self.try_negotiate_range(client_version) {
                return Ok(version);
            }
        }

        Err(HandshakeError::NoCompatibleVersion)
    }

    /// Try to negotiate a version from a range specification.
    ///
    /// Bolt version range format (bytes in network order):
    /// - [reserved, major, minor, range]
    /// - As u32 big-endian: 0x00_MM_mm_RR where:
    ///   - MM = major version
    ///   - mm = minor version
    ///   - RR = range (how many minor versions back to support)
    ///
    /// Example: 0x00040302 = major=4, minor=3, range=2 -> Bolt 4.3 supporting 4.3, 4.2, 4.1
    fn try_negotiate_range(&self, version_spec: u32) -> Option<BoltVersion> {
        // Parse format: 0x00_MM_mm_RR (big-endian)
        let major = ((version_spec >> 16) & 0xFF) as u16;
        let minor = ((version_spec >> 8) & 0xFF) as u16;
        let range = (version_spec & 0xFF) as u16;

        // Range of 0 means exact version only
        if range == 0 {
            return None;
        }

        // Try versions from minor down to (minor - range)
        for m in (minor.saturating_sub(range)..=minor).rev() {
            // Construct version value in our internal format: 0x00_MM_00_mm
            let version_value = ((major as u32) << 16) | (m as u32);
            if let Some(version) = BoltVersion::from_u32(version_value)
                && self.supported_versions.contains(&version)
            {
                return Some(version);
            }
        }

        None
    }

    /// Generate the server response bytes.
    ///
    /// Returns 4 bytes: the negotiated version or 0x00000000 if no compatible version.
    pub fn generate_response(result: &Result<HandshakeResult, HandshakeError>) -> [u8; 4] {
        match result {
            Ok(hr) => hr.version.to_bytes(),
            Err(_) => [0x00, 0x00, 0x00, 0x00],
        }
    }
}

impl Default for Handshake {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a client handshake message.
///
/// This is mainly useful for testing.
#[expect(dead_code)]
pub fn build_client_handshake(versions: &[BoltVersion]) -> [u8; 20] {
    let mut data = [0u8; 20];

    // Magic number
    data[0..4].copy_from_slice(&BOLT_MAGIC);

    // Version proposals (up to 4)
    for (i, version) in versions.iter().take(4).enumerate() {
        let offset = 4 + (i * 4);
        data[offset..offset + 4].copy_from_slice(&version.to_bytes());
    }

    data
}
