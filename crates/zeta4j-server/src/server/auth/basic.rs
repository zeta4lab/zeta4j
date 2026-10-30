//! Basic Authentication parsing.
//!
//! Implements HTTP Basic Authentication (RFC 7617) parsing.
//! Format: `Authorization: Basic base64(username:password)`

use base64::{Engine, engine::general_purpose::STANDARD};

use super::error::{AuthError, AuthResult};

/// Parsed Basic Auth credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicCredentials {
    /// Username from the credentials.
    pub username: String,
    /// Password from the credentials.
    pub password: String,
}

impl BasicCredentials {
    /// Creates new BasicCredentials.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Parses a Basic Auth header value.
    ///
    /// # Arguments
    /// * `header_value` - The full Authorization header value (e.g., "Basic dXNlcjpwYXNz")
    ///
    /// # Example
    /// ```ignore
    /// let creds = BasicCredentials::parse("Basic dXNlcjpwYXNz")?;
    /// assert_eq!(creds.username, "user");
    /// assert_eq!(creds.password, "pass");
    /// ```
    pub fn parse(header_value: &str) -> AuthResult<Self> {
        // Check for "Basic " prefix
        let encoded = header_value
            .strip_prefix("Basic ")
            .or_else(|| header_value.strip_prefix("basic "))
            .ok_or_else(|| AuthError::InvalidAuthHeader("Expected 'Basic ' prefix".to_string()))?;

        Self::parse_encoded(encoded)
    }

    /// Parses Base64-encoded credentials (without the "Basic " prefix).
    pub fn parse_encoded(encoded: &str) -> AuthResult<Self> {
        // Decode Base64
        let decoded = STANDARD
            .decode(encoded.trim())
            .map_err(|e| AuthError::InvalidBase64(e.to_string()))?;

        // Convert to UTF-8
        let decoded_str =
            String::from_utf8(decoded).map_err(|e| AuthError::InvalidUtf8(e.to_string()))?;

        // Split on first colon
        let (username, password) = decoded_str.split_once(':').ok_or_else(|| {
            AuthError::InvalidAuthHeader("Missing ':' separator in credentials".to_string())
        })?;

        Ok(Self {
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    /// Encodes credentials to a Basic Auth header value.
    ///
    /// # Example
    /// ```ignore
    /// let creds = BasicCredentials::new("user", "pass");
    /// assert_eq!(creds.encode(), "Basic dXNlcjpwYXNz");
    /// ```
    pub fn encode(&self) -> String {
        let combined = format!("{}:{}", self.username, self.password);
        let encoded = STANDARD.encode(combined.as_bytes());
        format!("Basic {}", encoded)
    }
}

/// Extracts Basic Auth credentials from an optional Authorization header.
pub fn extract_basic_auth(auth_header: Option<&str>) -> AuthResult<BasicCredentials> {
    let header = auth_header.ok_or(AuthError::MissingAuthHeader)?;
    BasicCredentials::parse(header)
}

/// Checks if a header value is a Basic Auth header.
pub fn is_basic_auth(header_value: &str) -> bool {
    header_value.starts_with("Basic ") || header_value.starts_with("basic ")
}
