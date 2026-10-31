// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Edition-neutral Cypher shell.
//!
//! 대화형 Cypher 쉘 구현
//!
//! - core: CypherShell 핵심 구현
//! - commands: 메타 명령 레지스트리
//! - parser: 입력 파서
//! - history: 명령 히스토리
//! - completion: 자동 완성
//! - readline: 라인 편집
//! - transaction: 트랜잭션 관리

mod commands;
mod completion;
mod core;
mod history;
mod parser;
mod readline;
mod transaction;

// Core Shell exports
pub use core::{
    CommandHistory, ConnectionMode, CypherShell, ParamValue, ParameterStore, QueryResult,
    QueryStats, ShellConfig, ShellConnection, ShellError,
};

// Commands exports
pub use commands::{CommandRegistry, CommandResult, MetaCommand};

// Parser exports
pub use parser::{ParsedCommand, ShellParser};

// History exports
pub use history::{HistoryError, SecureHistory};

// Completion exports
pub use completion::{
    CYPHER_AGGREGATES, CYPHER_CLAUSES, CYPHER_OPERATORS, CYPHER_PROCEDURES,
    CYPHER_SCALAR_FUNCTIONS, CompletionEngine, CompletionFormatter, CompletionKind,
    CompletionResult, META_COMMANDS, META_FORMAT_VALUES, META_SET_KEYS, SchemaInfo,
};

// Readline exports
pub use readline::{CypherHelper, ShellEditor, ShellReadError};

// Transaction exports
pub use transaction::{
    TransactionCommitInfo, TransactionError, TransactionManager, TransactionRollbackInfo,
    TransactionState,
};
