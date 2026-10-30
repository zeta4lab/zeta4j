//! CLI definition
//!
//! `z4j server` command arguments.

use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};
use zeta4j_common::RuntimeProfile;

use super::commands::stop::{DEFAULT_START_TIMEOUT, DEFAULT_STOP_TIMEOUT};
/// Zeta4j server controller
#[derive(ClapArgs, Debug)]
#[command(author = "Zeta4j Team")]
#[command(about = "Zeta4j Server Controller", long_about = None)]
pub struct Args {
    /// Home directory
    #[arg(long, env = "ZETA4J_HOME")]
    pub home: Option<PathBuf>,

    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Quiet mode (print errors only)
    #[arg(short = 'q', long)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: CtlCommand,
}

#[derive(Subcommand, Debug)]
pub enum CtlCommand {
    /// Start the server as a background daemon
    Start {
        /// Explicit path to the zeta4jd binary to run
        #[arg(long, env = "ZETA4J_SERVER_PATH")]
        server_path: Option<PathBuf>,

        /// Bind host
        #[arg(long)]
        host: Option<String>,

        /// HTTP port
        #[arg(long = "http-port")]
        http_port: Option<u16>,

        /// Bolt port
        #[arg(long)]
        bolt_port: Option<u16>,

        /// gRPC admin port
        #[arg(long)]
        grpc_port: Option<u16>,

        /// Startup timeout in seconds
        #[arg(long, default_value_t = DEFAULT_START_TIMEOUT)]
        timeout: u64,

        /// Stop any existing server or process using the ports before starting
        #[arg(short, long)]
        replace: bool,

        /// Server time zone (local = system time, utc)
        #[arg(long, value_parser = ["utc", "local"])]
        timezone: Option<String>,

        /// Disable authentication (development/testing only)
        #[arg(long)]
        no_auth: bool,

        /// Runtime security profile
        #[arg(long, env = "ZETA4J_RUNTIME_PROFILE")]
        profile: Option<RuntimeProfile>,
    },

    /// Stop the server
    Stop {
        /// Force stop (SIGKILL)
        #[arg(short, long)]
        force: bool,

        /// Graceful shutdown timeout in seconds
        #[arg(long, default_value_t = DEFAULT_STOP_TIMEOUT)]
        timeout: u64,
    },

    /// Restart the server
    Restart {
        /// Explicit path to the zeta4jd binary to run
        #[arg(long, env = "ZETA4J_SERVER_PATH")]
        server_path: Option<PathBuf>,

        /// Bind host
        #[arg(long)]
        host: Option<String>,

        /// HTTP port
        #[arg(long = "http-port")]
        http_port: Option<u16>,

        /// Bolt port
        #[arg(long)]
        bolt_port: Option<u16>,

        /// gRPC admin port
        #[arg(long)]
        grpc_port: Option<u16>,

        /// Force stop
        #[arg(short, long)]
        force: bool,

        /// Server time zone (local = system time, utc)
        #[arg(long, value_parser = ["utc", "local"])]
        timezone: Option<String>,

        /// Disable authentication (development/testing only)
        #[arg(long)]
        no_auth: bool,

        /// Runtime security profile
        #[arg(long, env = "ZETA4J_RUNTIME_PROFILE")]
        profile: Option<RuntimeProfile>,
    },

    /// Show server status
    Status,

    /// Show server logs
    Logs {
        /// Follow log output (like tail -f)
        #[arg(short, long)]
        follow: bool,

        /// Number of trailing lines to show
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
    },

    /// Run the server in the foreground (for development)
    Console {
        /// Explicit path to the zeta4jd binary to run
        #[arg(long, env = "ZETA4J_SERVER_PATH")]
        server_path: Option<PathBuf>,

        /// Bind host
        #[arg(long)]
        host: Option<String>,

        /// HTTP port
        #[arg(long = "http-port")]
        http_port: Option<u16>,

        /// Bolt port
        #[arg(long)]
        bolt_port: Option<u16>,

        /// gRPC admin port
        #[arg(long)]
        grpc_port: Option<u16>,

        /// Stop any existing server or process using the ports before starting
        #[arg(short, long)]
        replace: bool,

        /// Server time zone (local = system time, utc)
        #[arg(long, value_parser = ["utc", "local"])]
        timezone: Option<String>,

        /// Disable authentication (development/testing only)
        #[arg(long)]
        no_auth: bool,

        /// Runtime security profile
        #[arg(long, env = "ZETA4J_RUNTIME_PROFILE")]
        profile: Option<RuntimeProfile>,
    },

    /// Manage configuration
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Diagnose server configuration, resources, processes, and ports
    Doctor {
        /// Output directory for collected diagnostic data
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Manage the systemd service (Linux)
    #[command(subcommand)]
    Service(ServiceCommand),

    /// Initialize the storage
    Init {
        /// Overwrite existing configuration files
        #[arg(short, long)]
        force: bool,

        /// Reinitialize data and generated configuration in the current format
        #[arg(long)]
        clean: bool,

        /// Skip the confirmation prompt (use with --clean)
        #[arg(short = 'y', long)]
        yes: bool,

        /// Generate a one-time bootstrap credential envelope
        #[arg(long)]
        generate_bootstrap_credential: bool,

        /// New mode-0600 file for the generated credential
        #[arg(long, requires = "generate_bootstrap_credential")]
        bootstrap_output: Option<PathBuf>,
    },
}

/// Configuration commands
#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Check the configuration directory
    Check {
        /// Configuration directory path
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Print the current configuration
    Show,
    /// Generate default configuration files
    Init {
        /// Overwrite existing configuration files
        #[arg(short, long)]
        force: bool,
    },
}

/// systemd service commands
#[derive(Subcommand, Debug)]
pub enum ServiceCommand {
    /// Install the systemd service file
    Install {
        /// Service name
        #[arg(long, default_value = "zeta4j")]
        name: String,
        /// User to run the service as
        #[arg(long)]
        user: Option<String>,
        /// Explicit path to the zeta4jd binary to run
        #[arg(long, env = "ZETA4J_SERVER_PATH")]
        server_path: Option<PathBuf>,
    },
    /// Remove the systemd service file
    Uninstall {
        /// Service name
        #[arg(long, default_value = "zeta4j")]
        name: String,
    },
    /// Show systemd service status
    Status {
        /// Service name
        #[arg(long, default_value = "zeta4j")]
        name: String,
    },
}
