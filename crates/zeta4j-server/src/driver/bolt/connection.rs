// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bolt protocol connection for client-side use.
//!
//! Handles TCP connection, handshake, and message framing.

use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::codec::{Decoder, Encoder};

use crate::server::bolt::codec::BoltResponseCodec;
use crate::server::bolt::handshake::{BOLT_MAGIC, HANDSHAKE_RESPONSE_SIZE};
use crate::server::bolt::{BoltError, BoltRequest, BoltResponse, BoltResult, BoltVersion};

use super::SUPPORTED_VERSIONS;

/// Bolt connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoltConnectionState {
    /// Just created, not connected
    Disconnected,
    /// TCP connected, not handshaked
    Connected,
    /// Handshake completed
    Ready,
    /// Connection failed
    Failed,
    /// Closed
    Closed,
}

/// Client-side Bolt connection.
///
/// Manages TCP connection, performs handshake, and handles message framing.
pub struct BoltConnection {
    /// TCP stream
    stream: TcpStream,
    /// Response codec (decodes responses, encodes requests)
    codec: BoltResponseCodec,
    /// Read buffer
    read_buffer: BytesMut,
    /// Write buffer
    write_buffer: BytesMut,
    /// Negotiated protocol version
    protocol_version: Option<BoltVersion>,
    /// Connection state
    state: BoltConnectionState,
    /// Server address
    address: String,
}

impl BoltConnection {
    /// Connect to a Bolt server.
    pub async fn connect(address: &str) -> BoltResult<Self> {
        let stream = TcpStream::connect(address).await.map_err(|e| {
            BoltError::Connection(format!("Failed to connect to {}: {}", address, e))
        })?;

        // Enable TCP nodelay for lower latency
        stream.set_nodelay(true).ok();

        Ok(Self {
            stream,
            codec: BoltResponseCodec::new(),
            read_buffer: BytesMut::with_capacity(8192),
            write_buffer: BytesMut::with_capacity(8192),
            protocol_version: None,
            state: BoltConnectionState::Connected,
            address: address.to_string(),
        })
    }

    /// Perform Bolt handshake.
    ///
    /// Sends magic number and version proposals, receives negotiated version.
    pub async fn handshake(&mut self) -> BoltResult<BoltVersion> {
        if self.state != BoltConnectionState::Connected {
            return Err(BoltError::Protocol(format!(
                "Cannot handshake in state {:?}",
                self.state
            )));
        }

        // Build handshake message: magic (4 bytes) + 4 versions (16 bytes)
        let mut handshake_buf = [0u8; 20];
        handshake_buf[0..4].copy_from_slice(&BOLT_MAGIC);

        // Write version proposals (4 bytes each, big-endian: 0x00_MM_mm_RR)
        // where MM=major, mm=minor, RR=range
        for (i, (major, minor)) in SUPPORTED_VERSIONS.iter().enumerate() {
            let offset = 4 + i * 4;
            handshake_buf[offset] = 0; // reserved
            handshake_buf[offset + 1] = *major;
            handshake_buf[offset + 2] = *minor;
            handshake_buf[offset + 3] = 0; // range (0 for single version)
        }

        // Send handshake
        self.stream
            .write_all(&handshake_buf)
            .await
            .map_err(|e| BoltError::Connection(format!("Handshake write failed: {}", e)))?;

        // Read response (4 bytes)
        let mut response = [0u8; HANDSHAKE_RESPONSE_SIZE];
        self.stream
            .read_exact(&mut response)
            .await
            .map_err(|e| BoltError::Connection(format!("Handshake read failed: {}", e)))?;

        // Parse version response (format: [0, 0, minor, major] - Bolt protocol standard)
        let minor = response[2];
        let major = response[3];

        if major == 0 && minor == 0 {
            self.state = BoltConnectionState::Failed;
            return Err(BoltError::Protocol(
                "Server does not support any proposed Bolt version".to_string(),
            ));
        }

        // Convert (major, minor) to BoltVersion
        let version = BoltVersion::from_bytes(response).ok_or_else(|| {
            BoltError::Protocol(format!("Unsupported Bolt version: {}.{}", major, minor))
        })?;
        self.protocol_version = Some(version);
        self.state = BoltConnectionState::Ready;

        Ok(version)
    }

    /// Send a Bolt request message.
    pub async fn send(&mut self, request: BoltRequest) -> BoltResult<()> {
        if self.state != BoltConnectionState::Ready {
            return Err(BoltError::Protocol(format!(
                "Cannot send in state {:?}",
                self.state
            )));
        }

        // Encode request
        self.write_buffer.clear();
        self.codec
            .encode(request, &mut self.write_buffer)
            .map_err(|e| BoltError::Protocol(format!("Encode failed: {}", e)))?;

        // Send
        self.stream
            .write_all(&self.write_buffer)
            .await
            .map_err(|e| BoltError::Connection(format!("Send failed: {}", e)))?;

        self.stream
            .flush()
            .await
            .map_err(|e| BoltError::Connection(format!("Flush failed: {}", e)))?;

        Ok(())
    }

    /// Receive a Bolt response message.
    pub async fn recv(&mut self) -> BoltResult<BoltResponse> {
        if self.state != BoltConnectionState::Ready {
            return Err(BoltError::Protocol(format!(
                "Cannot receive in state {:?}",
                self.state
            )));
        }

        loop {
            // Try to decode from existing buffer
            if let Some(response) = self.codec.decode(&mut self.read_buffer)? {
                return Ok(response);
            }

            // Need more data
            let n = self
                .stream
                .read_buf(&mut self.read_buffer)
                .await
                .map_err(|e| BoltError::Connection(format!("Read failed: {}", e)))?;

            if n == 0 {
                self.state = BoltConnectionState::Closed;
                return Err(BoltError::Connection(
                    "Connection closed by server".to_string(),
                ));
            }
        }
    }

    /// Send a request and receive a single response.
    pub async fn request(&mut self, request: BoltRequest) -> BoltResult<BoltResponse> {
        self.send(request).await?;
        self.recv().await
    }

    /// Close the connection gracefully.
    pub async fn close(&mut self) -> BoltResult<()> {
        if self.state == BoltConnectionState::Ready {
            // Send GOODBYE
            let _ = self.send(BoltRequest::Goodbye).await;
        }

        self.state = BoltConnectionState::Closed;
        let _ = self.stream.shutdown().await;

        Ok(())
    }

    /// Get the negotiated protocol version.
    pub fn protocol_version(&self) -> Option<BoltVersion> {
        self.protocol_version
    }

    /// Get the connection state.
    pub fn state(&self) -> BoltConnectionState {
        self.state
    }

    /// Get the server address.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Check if connection is ready for messages.
    pub fn is_ready(&self) -> bool {
        self.state == BoltConnectionState::Ready
    }
}

impl std::fmt::Debug for BoltConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoltConnection")
            .field("address", &self.address)
            .field("state", &self.state)
            .field("protocol_version", &self.protocol_version)
            .finish()
    }
}
