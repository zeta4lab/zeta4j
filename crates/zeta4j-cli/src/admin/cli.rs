// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! CLI definition
//!
//! Command-line interface for `z4j admin`.

use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminCommandDescriptor {
    pub action: &'static str,
    pub rpc_method: &'static str,
}

/// Zeta4j database administration tool
#[derive(ClapArgs, Debug)]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(author = "Zeta4j Team")]
#[command(about = "Database Administration Tool", long_about = None)]
#[command(propagate_version = true)]
pub struct Args {
    /// gRPC server address
    #[arg(
        short = 'a',
        long,
        env = "ZETA4J_GRPC_ADDRESS",
        default_value = "127.0.0.1:9046"
    )]
    pub address: String,

    /// Connection timeout in milliseconds
    #[arg(long, env = "ZETA4J_CONNECT_TIMEOUT", default_value_t = 5_000)]
    pub connect_timeout: u64,

    /// Request timeout in milliseconds
    #[arg(long, env = "ZETA4J_REQUEST_TIMEOUT", default_value_t = 300_000)]
    pub request_timeout: u64,

    #[command(subcommand)]
    pub command: Command,
}

/// Import/export format
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum IoFormat {
    /// CSV
    Csv,
    /// JSON
    Json,
    /// NDJSON (Newline Delimited JSON)
    Ndjson,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create a database backup as a plain serialized bundle (not encrypted or signed)
    Backup {
        /// Database name
        #[arg(short, long, default_value = "zeta4j")]
        database: String,

        /// Output file path
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Restore a database from a backup bundle (not encrypted or signed)
    Restore {
        /// Database name
        #[arg(short, long, default_value = "zeta4j")]
        database: String,

        /// Input file path
        #[arg(short, long)]
        input: PathBuf,
    },

    /// Set the initial password
    #[command(name = "set-initial-password")]
    SetInitialPassword {
        /// Username
        #[arg(short, long)]
        username: String,

        /// Password (the interactive prompt is recommended)
        #[arg(short, long, conflicts_with = "password_env")]
        password: Option<String>,

        /// Read the password from an environment variable
        #[arg(long, env = "ZETA4J_PASSWORD", hide_env_values = true)]
        password_env: Option<String>,

        /// Require a password change on first login
        #[arg(long)]
        require_change: bool,
    },

    /// Show the server configuration
    #[command(name = "show-config")]
    ShowConfig,

    /// Show the server version
    Version,

    /// List databases
    #[command(name = "databases")]
    ListDatabases,

    /// Create a database
    #[command(name = "create-database")]
    CreateDatabase {
        /// Database name
        #[arg(short, long)]
        name: String,
    },

    /// Drop a database
    #[command(name = "drop-database")]
    DropDatabase {
        /// Database name
        #[arg(short, long)]
        name: String,

        /// Force the drop
        #[arg(short, long)]
        force: bool,
    },

    /// Drop a full-text index
    #[command(name = "drop-fulltext-index")]
    DropFulltextIndex {
        /// Database name
        #[arg(short, long, default_value = "zeta4j")]
        database: String,

        /// Index name
        #[arg(short = 'n', long)]
        name: String,
    },

    /// Show database status
    #[command(name = "database-status")]
    DatabaseStatus {
        /// Database name
        #[arg(short, long, default_value = "zeta4j")]
        name: String,
    },

    /// Start a database (set it online)
    #[command(name = "start-database")]
    StartDatabase {
        /// Database name
        #[arg(short, long)]
        name: String,
    },

    /// Stop a database (set it offline)
    #[command(name = "stop-database")]
    StopDatabase {
        /// Database name
        #[arg(short, long)]
        name: String,
    },

    /// Show system status
    Status {
        /// Show detailed status for each service
        #[arg(short, long)]
        verbose: bool,
    },

    /// List users
    #[command(name = "users")]
    ListUsers,

    /// Create a user
    #[command(name = "create-user")]
    CreateUser {
        /// Username
        #[arg(short, long)]
        username: String,

        /// Password (the interactive prompt is recommended)
        #[arg(short, long, conflicts_with = "password_env")]
        password: Option<String>,

        /// Read the password from an environment variable
        #[arg(long, env = "ZETA4J_PASSWORD", hide_env_values = true)]
        password_env: Option<String>,
    },

    /// Delete a user
    #[command(name = "delete-user")]
    DeleteUser {
        /// Username
        #[arg(short, long)]
        username: String,
    },

    /// Persisted service API key lifecycle
    #[command(name = "api-key")]
    ApiKey {
        #[command(subcommand)]
        command: ApiKeyCommand,
    },

    /// Authenticated credential session lifecycle
    #[command(name = "session")]
    CredentialSession {
        #[command(subcommand)]
        command: CredentialSessionCommand,
    },

    /// Credential signing key lifecycle
    #[command(name = "signing-key")]
    SigningKey {
        #[command(subcommand)]
        command: SigningKeyCommand,
    },

    /// Import node data (CSV, JSON, NDJSON)
    Import {
        /// Input file path
        #[arg(short, long)]
        input: PathBuf,

        /// Import format
        #[arg(short, long, value_enum, default_value = "csv")]
        format: IoFormat,

        /// Data type
        #[arg(short = 't', long, default_value = "nodes", value_parser = ["nodes"])]
        data_type: String,

        /// Label for the created nodes
        #[arg(short, long)]
        label: Option<String>,

        /// Transfer chunk size in KiB
        #[arg(long, default_value = "1024")]
        chunk_size_kib: usize,

        /// CSV delimiter
        #[arg(long, default_value = ",")]
        delimiter: char,

        /// Verbose output
        #[arg(long)]
        verbose: bool,
    },

    /// Export node or edge data (CSV, JSON, NDJSON)
    Export {
        /// Output file path
        #[arg(short, long)]
        output: PathBuf,

        /// Export format
        #[arg(short, long, value_enum, default_value = "csv")]
        format: IoFormat,

        /// Data type
        #[arg(short = 't', long, default_value = "nodes", value_parser = ["nodes", "edges"])]
        data_type: String,

        /// Filter by node label or edge type
        #[arg(short, long)]
        label: Option<String>,

        /// CSV delimiter
        #[arg(long, default_value = ",")]
        delimiter: char,

        /// Verbose output
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ApiKeyCommand {
    /// List API key metadata (never secret material)
    List,
    /// Create an API key and print its secret once
    Create {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "http", value_parser = ["http"])]
        audience: String,
        #[arg(long = "scope", default_value = "api", value_parser = ["api", "administration"])]
        scopes: Vec<String>,
        #[arg(long, default_value_t = 2_592_000)]
        expires_in_seconds: u64,
    },
    /// Rotate an API key and print the successor secret once
    Rotate {
        #[arg(long)]
        id: String,
        #[arg(long, default_value_t = 3_600)]
        overlap_seconds: u64,
        #[arg(long, default_value_t = 2_592_000)]
        expires_in_seconds: u64,
    },
    /// Revoke an API key immediately
    Revoke {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum CredentialSessionCommand {
    /// List durable credential sessions
    List,
    /// Revoke a durable credential session
    Revoke {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SigningKeyCommand {
    /// List public signing-key lifecycle metadata
    List,
    /// Mark a key compromised and revoke every dependent session
    Compromise {
        #[arg(long = "key-version")]
        key_version: u64,
    },
}

impl Command {
    pub const fn command_descriptor(&self) -> AdminCommandDescriptor {
        match self {
            Self::Backup { .. } => descriptor("cli.admin.backup", "Backup"),
            Self::Restore { .. } => descriptor("cli.admin.restore", "Restore"),
            Self::SetInitialPassword { .. } => {
                descriptor("cli.admin.set-initial-password", "SetInitialPassword")
            }
            Self::ShowConfig => descriptor("cli.admin.show-config", "GetConfig"),
            Self::Version => descriptor("cli.admin.version", "GetVersion"),
            Self::ListDatabases => descriptor("cli.admin.databases", "ListDatabases"),
            Self::CreateDatabase { .. } => {
                descriptor("cli.admin.create-database", "CreateDatabase")
            }
            Self::DropDatabase { .. } => descriptor("cli.admin.drop-database", "DropDatabase"),
            Self::DropFulltextIndex { .. } => {
                descriptor("cli.admin.drop-fulltext-index", "DropFulltextIndex")
            }
            Self::DatabaseStatus { .. } => {
                descriptor("cli.admin.database-status", "GetDatabaseStatus")
            }
            Self::StartDatabase { .. } => descriptor("cli.admin.start-database", "StartDatabase"),
            Self::StopDatabase { .. } => descriptor("cli.admin.stop-database", "StopDatabase"),
            Self::Status { .. } => descriptor("cli.admin.status", "GetSystemStatus"),
            Self::ListUsers => descriptor("cli.admin.users", "ListUsers"),
            Self::CreateUser { .. } => descriptor("cli.admin.create-user", "CreateUser"),
            Self::DeleteUser { .. } => descriptor("cli.admin.delete-user", "DeleteUser"),
            Self::ApiKey { command } => match command {
                ApiKeyCommand::List => descriptor("cli.admin.api-key.list", "ListApiKeys"),
                ApiKeyCommand::Create { .. } => {
                    descriptor("cli.admin.api-key.create", "CreateApiKey")
                }
                ApiKeyCommand::Rotate { .. } => {
                    descriptor("cli.admin.api-key.rotate", "RotateApiKey")
                }
                ApiKeyCommand::Revoke { .. } => {
                    descriptor("cli.admin.api-key.revoke", "RevokeApiKey")
                }
            },
            Self::CredentialSession { command } => match command {
                CredentialSessionCommand::List => {
                    descriptor("cli.admin.session.list", "ListCredentialSessions")
                }
                CredentialSessionCommand::Revoke { .. } => {
                    descriptor("cli.admin.session.revoke", "RevokeCredentialSession")
                }
            },
            Self::SigningKey { command } => match command {
                SigningKeyCommand::List => {
                    descriptor("cli.admin.signing-key.list", "ListSigningKeys")
                }
                SigningKeyCommand::Compromise { .. } => {
                    descriptor("cli.admin.signing-key.compromise", "CompromiseSigningKey")
                }
            },
            Self::Import { .. } => descriptor("cli.admin.import", "Import"),
            Self::Export { .. } => descriptor("cli.admin.export", "Export"),
        }
    }
}

const fn descriptor(action: &'static str, rpc_method: &'static str) -> AdminCommandDescriptor {
    AdminCommandDescriptor { action, rpc_method }
}

#[cfg(test)]
mod tests {
    use clap::Args as _;

    use super::Args;

    fn help_text() -> String {
        let mut command = Args::augment_args(clap::Command::new("admin"));
        let mut text = command.render_long_help().to_string();
        for name in ["backup", "restore"] {
            if let Some(subcommand) = command.find_subcommand_mut(name) {
                text.push_str(&subcommand.render_long_help().to_string());
            }
        }
        text
    }

    #[test]
    fn backup_help_describes_plain_serialized_bundles() {
        let help = help_text();
        assert!(help.contains("not encrypted"), "{help}");
    }
}
