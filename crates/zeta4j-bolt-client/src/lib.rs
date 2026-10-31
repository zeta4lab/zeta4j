// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Edition-neutral Bolt client and interactive shell.

mod bolt_client;
mod formatter;
pub mod shell;

pub use bolt_client::{BoltClient, BoltClientError, BoltClientResult, ReconnectConfig};
pub use formatter::{
    CsvFormatter, JsonFormatter, OutputFormat, OutputFormatter, Paginator, PlainFormatter,
    TableFormatter,
};
pub use shell::{CypherShell, ShellConfig};
