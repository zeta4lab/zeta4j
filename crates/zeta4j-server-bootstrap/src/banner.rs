// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server banner and startup information
//!
//! 서버 배너 출력

use std::net::SocketAddr;

use log::info;

use zeta4j_common::Edition;

/// Cargo.toml에서 버전 가져오기
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Print the ASCII art banner
pub fn print_banner(edition: Edition) {
    println!(
        r#"
╔═══════════════════════════════════════════════════════════════╗
║                                                               ║
║   ███████╗███████╗████████╗ █████╗ ██╗  ██╗ ██████╗           ║
║   ╚══███╔╝██╔════╝╚══██╔══╝██╔══██╗██║  ██║██╔════╝           ║
║     ███╔╝ █████╗     ██║   ███████║███████║██║  ███╗          ║
║    ███╔╝  ██╔══╝     ██║   ██╔══██║╚════██║██║   ██║          ║
║   ███████╗███████╗   ██║   ██║  ██║     ██║╚██████╔╝          ║
║   ╚══════╝╚══════╝   ╚═╝   ╚═╝  ╚═╝     ╚═╝ ╚═════╝           ║
║                                                               ║
║   {} Edition v{}
║   {}
║                                                               ║
╚═══════════════════════════════════════════════════════════════╝
"#,
        edition.name(),
        VERSION,
        edition.description()
    );
}

/// Server startup information
pub struct StartupInfo {
    pub edition: Edition,
    pub http_addr: Option<SocketAddr>,
    pub secure_http_addr: Option<SocketAddr>,
    pub bolt_addr: SocketAddr,
    pub grpc_addr: Option<SocketAddr>,
    pub data_dir: String,
    pub log_dir: String,
}

impl StartupInfo {
    /// Create new startup info
    pub fn new(edition: Edition) -> Self {
        Self {
            edition,
            http_addr: None,
            secure_http_addr: None,
            bolt_addr: SocketAddr::from(([127, 0, 0, 1], 9045)),
            grpc_addr: None,
            data_dir: String::new(),
            log_dir: String::new(),
        }
    }

    /// Print server startup information
    pub fn print(&self) {
        info!("===========================================");
        info!(
            "  Zeta4j {} Server v{} Started",
            self.edition.name(),
            VERSION
        );
        info!("===========================================");
        info!(
            "  Edition:   {} ({})",
            self.edition.name(),
            self.edition.description()
        );

        if let Some(addr) = &self.http_addr {
            info!("  HTTP API:  http://{}", addr);
        }

        if let Some(addr) = &self.secure_http_addr {
            info!("  HTTPS API: https://{}", addr);
        }

        info!("  Bolt:      bolt://{}", self.bolt_addr);

        if let Some(addr) = &self.grpc_addr {
            info!("  gRPC:      {}", addr);
        }
        info!("  Data:      {}", self.data_dir);
        info!("  Logs:      {}", self.log_dir);
        info!("===========================================");

        // Edition-specific features
        self.print_edition_features();

        info!("");
        info!("Press Ctrl+C to stop the server");
    }

    fn print_edition_features(&self) {
        info!("");
        info!("Features:");

        info!("  - Graph Engine with ACID Transactions");
        info!("  - Bolt Protocol & HTTP API");
        info!("  - Multi-tenancy Support");
        info!("  - HNSW Vector Index");
        info!("  - Fulltext Index");
    }
}
