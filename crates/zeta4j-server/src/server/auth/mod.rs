// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Authentication for the Zeta4j graph database server.
//!
//! Adapters resolve passwords, bearer tokens and API keys into the canonical
//! authenticated principal. Every authenticated principal has the same access.

pub mod apikey;
pub mod basic;
pub mod credential_token;
pub mod error;
pub mod middleware;
pub mod password;

// Public re-exports
pub use apikey::{ApiKeyLayer, ApiKeyService, X_API_KEY_HEADER, extract_api_key};
pub use basic::{BasicCredentials, extract_basic_auth, is_basic_auth};
pub use credential_token::{CredentialTokenService, Ed25519Signer, SignerReference};
pub use error::{AuthError, AuthResult};
pub use middleware::{AuthConfig, AuthLayer, AuthService, AuthenticatedUser};
pub use password::{hash_password, verify_password, verify_password_outcome};
