//! Bolt protocol codec for tokio_util.
//!
//! Implements chunked message framing as per Bolt protocol specification.
//! Messages are split into chunks with a 2-byte length prefix.

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use super::BoltError;
use super::message::{BoltRequest, BoltResponse};
use super::packstream::{PackStreamValue, decode, encode};

/// Maximum chunk size (16KB)
pub const MAX_CHUNK_SIZE: usize = 16384;

/// Minimum chunk size
pub const MIN_CHUNK_SIZE: usize = 8;

/// End of message marker (0x00 0x00)
pub const END_MARKER: [u8; 2] = [0x00, 0x00];

/// Bolt message codec for framing.
#[derive(Debug)]
pub struct BoltCodec {
    /// Maximum message size
    max_message_size: usize,
    /// Buffer for accumulating chunks
    message_buffer: BytesMut,
    /// State: are we in the middle of a message?
    in_message: bool,
}

impl BoltCodec {
    /// Create a new codec with default settings.
    pub fn new() -> Self {
        Self {
            max_message_size: 16 * 1024 * 1024, // 16MB default
            message_buffer: BytesMut::with_capacity(4096),
            in_message: false,
        }
    }

    /// Create a codec with custom max message size.
    pub fn with_max_size(max_message_size: usize) -> Self {
        Self {
            max_message_size,
            message_buffer: BytesMut::with_capacity(4096),
            in_message: false,
        }
    }

    /// Encode a message into chunks.
    fn encode_chunked(&self, data: &[u8], dst: &mut BytesMut) {
        let mut offset = 0;

        while offset < data.len() {
            let remaining = data.len() - offset;
            let chunk_size = remaining.min(MAX_CHUNK_SIZE);

            // Write chunk header (2-byte big-endian length)
            dst.put_u16(chunk_size as u16);
            // Write chunk data
            dst.put_slice(&data[offset..offset + chunk_size]);

            offset += chunk_size;
        }

        // Write end marker
        dst.put_slice(&END_MARKER);
    }
}

impl Default for BoltCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for BoltCodec {
    type Item = PackStreamValue;
    type Error = BoltError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            // Need at least 2 bytes for chunk header
            if src.len() < 2 {
                return Ok(None);
            }

            // Peek at chunk size
            let chunk_size = u16::from_be_bytes([src[0], src[1]]) as usize;

            // Check for end marker
            if chunk_size == 0 {
                src.advance(2); // Consume end marker

                if self.message_buffer.is_empty() {
                    // Empty message - could be NOOP, skip
                    continue;
                }

                // Decode complete message
                let message_data = self.message_buffer.split();
                self.in_message = false;

                let value = decode(&message_data).map_err(BoltError::PackStream)?;

                return Ok(Some(value));
            }

            // Need chunk header + chunk data
            if src.len() < 2 + chunk_size {
                return Ok(None);
            }

            // Check message size limit
            if self.message_buffer.len() + chunk_size > self.max_message_size {
                return Err(BoltError::MessageTooLarge {
                    size: self.message_buffer.len() + chunk_size,
                    max: self.max_message_size,
                });
            }

            // Consume chunk header
            src.advance(2);

            // Append chunk data to message buffer
            self.message_buffer.extend_from_slice(&src[..chunk_size]);
            src.advance(chunk_size);
            self.in_message = true;
        }
    }
}

impl Encoder<PackStreamValue> for BoltCodec {
    type Error = BoltError;

    fn encode(&mut self, item: PackStreamValue, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let encoded = encode(&item).map_err(BoltError::PackStream)?;
        self.encode_chunked(&encoded, dst);
        Ok(())
    }
}

/// Codec specifically for Bolt request messages.
#[derive(Debug, Default)]
pub struct BoltRequestCodec {
    inner: BoltCodec,
}

impl BoltRequestCodec {
    /// Create a new request codec.
    pub fn new() -> Self {
        Self {
            inner: BoltCodec::new(),
        }
    }
}

impl Decoder for BoltRequestCodec {
    type Item = BoltRequest;
    type Error = BoltError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.inner.decode(src)? {
            Some(value) => {
                let structure = value
                    .as_structure()
                    .ok_or_else(|| BoltError::Protocol("Expected structure".to_string()))?;

                let request =
                    BoltRequest::from_structure(structure).map_err(BoltError::PackStream)?;

                Ok(Some(request))
            }
            None => Ok(None),
        }
    }
}

impl Encoder<BoltResponse> for BoltRequestCodec {
    type Error = BoltError;

    fn encode(&mut self, item: BoltResponse, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let structure = item.to_structure();
        let value = PackStreamValue::Structure(structure);
        self.inner.encode(value, dst)
    }
}

/// Codec specifically for Bolt response messages (client-side).
#[derive(Debug, Default)]
pub struct BoltResponseCodec {
    inner: BoltCodec,
}

impl BoltResponseCodec {
    /// Create a new response codec.
    pub fn new() -> Self {
        Self {
            inner: BoltCodec::new(),
        }
    }
}

impl Decoder for BoltResponseCodec {
    type Item = BoltResponse;
    type Error = BoltError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.inner.decode(src)? {
            Some(value) => {
                let structure = value
                    .as_structure()
                    .ok_or_else(|| BoltError::Protocol("Expected structure".to_string()))?;

                let response =
                    BoltResponse::from_structure(structure).map_err(BoltError::PackStream)?;

                Ok(Some(response))
            }
            None => Ok(None),
        }
    }
}

impl Encoder<BoltRequest> for BoltResponseCodec {
    type Error = BoltError;

    fn encode(&mut self, item: BoltRequest, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let structure = item.to_structure();
        let value = PackStreamValue::Structure(structure);
        self.inner.encode(value, dst)
    }
}
