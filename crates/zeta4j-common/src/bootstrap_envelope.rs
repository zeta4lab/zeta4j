// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Versioned one-time bootstrap credential envelope format.

pub const BOOTSTRAP_ENVELOPE_VERSION: u8 = 1;
pub const DEFAULT_BOOTSTRAP_TTL_SECS: u64 = 15 * 60;

/// Render the file created by `z4j server init`.
///
/// The caller must write this only to an explicitly selected mode-0600 file.
pub fn render_bootstrap_envelope(username: &str, password: &str, expires_at_unix: u64) -> String {
    format!(
        "version = {BOOTSTRAP_ENVELOPE_VERSION}\nusername = {username:?}\npassword = {password:?}\nexpires_at_unix = {expires_at_unix}\n"
    )
}
