// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Canonical generated server configuration templates.

pub const DEFAULT_CORE_TOML: &str = r#"# Zeta4j Core Configuration

[logging]
level = "info"
format = "json"
directory = "~/.zeta4j/logs"

[storage]
data_dir = "~/.zeta4j/databases"
# Optional positive per-database admission limit. Omit to disable.
# memory_limit_bytes = 1073741824

[transaction]
timeout_ms = 5000
lock_wait_ms = 1000

[wal]
max_segment_size = 67108864
max_segments = 10
flush_interval = 100
"#;

pub const DEFAULT_SERVER_TOML: &str = r#"# Zeta4j Server Configuration

[runtime]
profile = "production"

[http]
bind = "127.0.0.1"
port = 9044
max_connections = 10000
request_timeout_secs = 30
max_body_size = 16777216
compression = true
keep_alive_timeout_secs = 60
timezone = "local"

[bolt]
bind = "127.0.0.1"
port = 9045
max_connections = 1000
connection_timeout_secs = 30
default_database = "zeta4j"

[grpc]
bind = "127.0.0.1"
port = 9046

[transport]
mode = "plaintext"
server_cert_ref = ""
server_key_ref = ""

[auth]
enabled = true
jwt_signing_key_file = ""
jwt_key_id = "key-1"
jwt_key_version = 1
access_token_ttl_secs = 3600
refresh_token_ttl_secs = 604800
"#;
