// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! M8.3: Meta Commands
//!
//! 셸 메타 명령 (:help, :sysinfo, :schema 등)

use std::path::PathBuf;

use super::core::{CypherShell, ParamValue, ShellError};
use crate::formatter::OutputFormat;

// ============================================================================
// MetaCommand - 메타 명령 타입
// ============================================================================

/// 메타 명령
#[derive(Debug, Clone)]
pub enum MetaCommand {
    /// 도움말
    Help,
    /// 시스템 정보
    Sysinfo,
    /// 스키마 조회
    Schema,
    /// 데이터베이스 전환
    Use(String),
    /// 히스토리
    History(Option<usize>),
    /// 화면 클리어
    Clear,
    /// 설정 변경
    Set(String, String),
    /// 파라미터 설정
    Param(String, String),
    /// 파라미터 목록
    ParamList,
    /// 파라미터 클리어
    ParamClear,
    /// 출력 포맷 변경
    Format(String),
    /// 파일 실행
    Source(String),
    /// 연결
    Connect(Option<String>),
    /// 연결 해제
    Disconnect,
    /// 버전 정보
    Version,
    /// 데이터베이스 목록
    Databases,
    /// 명시적 트랜잭션 시작
    Begin,
    /// 현재 트랜잭션 커밋
    Commit,
    /// 현재 트랜잭션 롤백 또는 실패 세션 초기화
    Rollback,
    /// 현재 트랜잭션 상태
    TxStatus,
    /// 쿼리 타임아웃 (초 단위, None=현재 값 표시)
    Timeout(Option<u64>),
    /// EXPLAIN 쿼리 실행 계획
    Explain(String),
    /// PROFILE 쿼리 실행 (실제 통계 포함)
    Profile(String),
    /// 페이지 크기 설정 (None=현재 값 표시)
    Page(Option<usize>),
    /// 다음 페이지 표시
    More,
    /// 결과 파일로 내보내기
    Export(String),
}

// ============================================================================
// CommandResult - 명령 결과
// ============================================================================

/// 명령 실행 결과
#[derive(Debug)]
pub enum CommandResult {
    /// 성공 (출력 메시지)
    Success(Option<String>),
    /// 에러
    Error(String),
    /// 종료
    Exit,
}

// ============================================================================
// CommandRegistry - 명령 레지스트리
// ============================================================================

/// 명령 레지스트리
pub struct CommandRegistry;

impl CommandRegistry {
    /// 새 레지스트리 생성
    pub fn new() -> Self {
        Self
    }

    /// 명령 실행
    pub fn execute(
        &self,
        cmd: MetaCommand,
        shell: &mut CypherShell,
    ) -> Result<CommandResult, ShellError> {
        match cmd {
            MetaCommand::Help => Ok(CommandResult::Success(Some(self.help_text()))),
            MetaCommand::Sysinfo => self.sysinfo(shell),
            MetaCommand::Schema => self.schema(shell),
            MetaCommand::Use(db) => self.use_database(shell, &db),
            MetaCommand::History(count) => self.history(shell, count),
            MetaCommand::Clear => self.clear(),
            MetaCommand::Set(key, value) => self.set(shell, &key, &value),
            MetaCommand::Param(name, value) => self.param(shell, &name, &value),
            MetaCommand::ParamList => self.param_list(shell),
            MetaCommand::ParamClear => self.param_clear(shell),
            MetaCommand::Format(fmt) => self.format(shell, &fmt),
            MetaCommand::Source(path) => self.source(shell, &path),
            MetaCommand::Connect(addr) => self.connect(shell, addr.as_deref()),
            MetaCommand::Disconnect => self.disconnect(shell),
            MetaCommand::Version => Ok(CommandResult::Success(Some(self.version_text()))),
            MetaCommand::Databases => self.databases(shell),
            MetaCommand::Begin => self.begin(shell),
            MetaCommand::Commit => self.commit(shell),
            MetaCommand::Rollback => self.rollback(shell),
            MetaCommand::TxStatus => self.tx_status(shell),
            MetaCommand::Timeout(secs) => self.timeout(shell, secs),
            MetaCommand::Explain(query) => self.explain(shell, &query),
            MetaCommand::Profile(query) => self.profile(shell, &query),
            MetaCommand::Page(size) => self.page(shell, size),
            MetaCommand::More => self.more(shell),
            MetaCommand::Export(path) => self.export(shell, &path),
        }
    }

    /// 도움말 텍스트
    fn help_text(&self) -> String {
        r#"
Available commands:
  :help, :?           Show this help message
  :exit, :quit, :q    Exit the shell
  :sysinfo            Show system information
  :schema             Show database schema
  :use <database>     Switch to a database
  :history [n]        Show command history (last n entries)
  :clear              Clear the screen
  :set <key> <value>  Set a configuration option
  :param <n> <v>      Set a query parameter
  :param              List all parameters
  :param clear        Clear all parameters
  :format <fmt>       Set output format (table, csv, json, plain)
  :source <file>      Execute commands from a file
  :connect [addr]     Connect to server
  :disconnect         Disconnect from server
  :version            Show version information
  :databases          List available databases
  :begin              Begin an explicit transaction
  :commit             Commit the current transaction
  :rollback           Roll back the current transaction
  :tx-status          Show transaction state
  :timeout [secs]     Set/show query timeout (0=unlimited)
  :explain <query>    Show query execution plan
  :profile <query>    Execute query and show actual statistics
  :page [size]        Set/show page size (0=unlimited)
  :more               Show next page of results
  :export <file>      Export last result to file

Query execution:
  Type a Cypher query and press Enter to execute
  Multi-line queries: press Enter to continue, end with semicolon (;)

Examples:
  MATCH (n) RETURN n LIMIT 10;
  :param name 'Alice'
  MATCH (n {name: $name}) RETURN n;
"#
        .to_string()
    }

    /// 버전 정보
    fn version_text(&self) -> String {
        format!(
            "Zeta4j v{}\nCypher Shell (cypher-shell compatible)\nRust {}",
            env!("CARGO_PKG_VERSION"),
            rustc_version_info()
        )
    }

    /// 시스템 정보
    fn sysinfo(&self, shell: &CypherShell) -> Result<CommandResult, ShellError> {
        let config = shell.config();

        let info = format!(
            r#"
System Information:
  Server:     {}
  Database:   {}
  Connected:  {}
  Format:     {}
  Color:      {}

Parameters: {}
History:    {} entries
"#,
            config.address,
            config.database,
            if shell.is_connected() { "Yes" } else { "No" },
            config.format,
            if config.color { "Yes" } else { "No" },
            shell.parameters().all().len(),
            shell.history().len()
        );

        Ok(CommandResult::Success(Some(info)))
    }

    /// 스키마 조회
    fn schema(&self, shell: &mut CypherShell) -> Result<CommandResult, ShellError> {
        if !shell.is_connected() {
            return Err(ShellError::ConnectionError("Not connected".to_string()));
        }

        let params = serde_json::Map::new();
        let mut output = String::from("Schema:\n");

        // 1. 라벨 조회
        output.push_str("\n  Labels:\n");
        match shell.run_internal_query("MATCH (n) RETURN DISTINCT labels(n) AS labels", &params) {
            Ok(result) => {
                let mut labels: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                for row in &result.rows {
                    if let Some(serde_json::Value::Array(label_arr)) = row.values.first() {
                        for label in label_arr {
                            if let serde_json::Value::String(s) = label {
                                labels.insert(s.clone());
                            }
                        }
                    }
                }
                if labels.is_empty() {
                    output.push_str("    (no labels)\n");
                } else {
                    let mut sorted: Vec<_> = labels.into_iter().collect();
                    sorted.sort();
                    for label in sorted {
                        output.push_str(&format!("    - {}\n", label));
                    }
                }
            }
            Err(_) => {
                output.push_str("    (unable to retrieve labels)\n");
            }
        }

        // 2. 관계 타입 조회
        output.push_str("\n  Relationship Types:\n");
        match shell.run_internal_query("MATCH ()-[r]->() RETURN DISTINCT type(r) AS type", &params)
        {
            Ok(result) => {
                let mut types: std::collections::HashSet<String> = std::collections::HashSet::new();
                for row in &result.rows {
                    if let Some(serde_json::Value::String(t)) = row.values.first() {
                        types.insert(t.clone());
                    }
                }
                if types.is_empty() {
                    output.push_str("    (no relationship types)\n");
                } else {
                    let mut sorted: Vec<_> = types.into_iter().collect();
                    sorted.sort();
                    for rel_type in sorted {
                        output.push_str(&format!("    - {}\n", rel_type));
                    }
                }
            }
            Err(_) => {
                output.push_str("    (unable to retrieve relationship types)\n");
            }
        }

        // 3. 프로퍼티 키 조회 (노드)
        output.push_str("\n  Property Keys (sample from nodes):\n");
        match shell.run_internal_query(
            "MATCH (n) RETURN DISTINCT keys(n) AS keys LIMIT 100",
            &params,
        ) {
            Ok(result) => {
                let mut prop_keys: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                for row in &result.rows {
                    if let Some(serde_json::Value::Array(keys_arr)) = row.values.first() {
                        for key in keys_arr {
                            if let serde_json::Value::String(s) = key {
                                prop_keys.insert(s.clone());
                            }
                        }
                    }
                }
                if prop_keys.is_empty() {
                    output.push_str("    (no properties)\n");
                } else {
                    let mut sorted: Vec<_> = prop_keys.into_iter().collect();
                    sorted.sort();
                    for key in sorted {
                        output.push_str(&format!("    - {}\n", key));
                    }
                }
            }
            Err(_) => {
                output.push_str("    (unable to retrieve property keys)\n");
            }
        }

        // 4. 노드/관계 카운트
        output.push_str("\n  Statistics:\n");
        if let Ok(result) =
            shell.run_internal_query("MATCH (n) RETURN count(n) AS node_count", &params)
            && let Some(row) = result.rows.first()
            && let Some(count) = row.values.first()
        {
            output.push_str(&format!("    - Total Nodes: {}\n", count));
        }
        if let Ok(result) =
            shell.run_internal_query("MATCH ()-[r]->() RETURN count(r) AS rel_count", &params)
            && let Some(row) = result.rows.first()
            && let Some(count) = row.values.first()
        {
            output.push_str(&format!("    - Total Relationships: {}\n", count));
        }

        Ok(CommandResult::Success(Some(output)))
    }

    /// 데이터베이스 전환
    fn use_database(
        &self,
        shell: &mut CypherShell,
        database: &str,
    ) -> Result<CommandResult, ShellError> {
        shell.use_database(database)?;
        Ok(CommandResult::Success(Some(format!(
            "Switched to database '{}'",
            database
        ))))
    }

    /// 히스토리 조회
    fn history(
        &self,
        shell: &CypherShell,
        count: Option<usize>,
    ) -> Result<CommandResult, ShellError> {
        let entries = shell.history().entries();
        let count = count.unwrap_or(20);

        let mut output = String::from("Command History:\n");
        let start = entries.len().saturating_sub(count);

        for (i, entry) in entries.iter().skip(start).enumerate() {
            output.push_str(&format!("  {}: {}\n", start + i + 1, entry));
        }

        Ok(CommandResult::Success(Some(output)))
    }

    /// 화면 클리어
    fn clear(&self) -> Result<CommandResult, ShellError> {
        // ANSI 클리어 코드
        Ok(CommandResult::Success(Some("\x1b[2J\x1b[H".to_string())))
    }

    /// 설정 변경
    fn set(
        &self,
        shell: &mut CypherShell,
        key: &str,
        value: &str,
    ) -> Result<CommandResult, ShellError> {
        match key.to_lowercase().as_str() {
            "format" => {
                if let Some(fmt) = OutputFormat::parse(value) {
                    shell.config_mut().format = fmt;
                    Ok(CommandResult::Success(Some(format!(
                        "Format set to {}",
                        fmt
                    ))))
                } else {
                    Err(ShellError::CommandError(format!(
                        "Unknown format: {}",
                        value
                    )))
                }
            }
            "color" => {
                let enabled = match value.to_lowercase().as_str() {
                    "true" | "yes" | "on" | "1" => true,
                    "false" | "no" | "off" | "0" => false,
                    _ => {
                        return Err(ShellError::CommandError(
                            "Value must be true/false".to_string(),
                        ));
                    }
                };
                shell.config_mut().color = enabled;
                Ok(CommandResult::Success(Some(format!(
                    "Color {}",
                    if enabled { "enabled" } else { "disabled" }
                ))))
            }
            "page_size" => {
                let size: usize = value
                    .parse()
                    .map_err(|_| ShellError::CommandError("Invalid page size".to_string()))?;
                shell.config_mut().page_size = if size == 0 { None } else { Some(size) };
                Ok(CommandResult::Success(Some(format!(
                    "Page size set to {}",
                    if size == 0 {
                        "unlimited".to_string()
                    } else {
                        size.to_string()
                    }
                ))))
            }
            _ => Err(ShellError::CommandError(format!(
                "Unknown setting: {}",
                key
            ))),
        }
    }

    /// 파라미터 설정
    fn param(
        &self,
        shell: &mut CypherShell,
        name: &str,
        value: &str,
    ) -> Result<CommandResult, ShellError> {
        let param_value = ParamValue::parse(value);
        shell.parameters_mut().set(name, param_value.clone());
        Ok(CommandResult::Success(Some(format!(
            "Parameter {} = {}",
            name, param_value
        ))))
    }

    /// 파라미터 목록
    fn param_list(&self, shell: &CypherShell) -> Result<CommandResult, ShellError> {
        let params = shell.parameters().all();

        if params.is_empty() {
            return Ok(CommandResult::Success(Some(
                "No parameters set".to_string(),
            )));
        }

        let mut output = String::from("Parameters:\n");
        for (name, value) in params {
            output.push_str(&format!("  ${} = {}\n", name, value));
        }

        Ok(CommandResult::Success(Some(output)))
    }

    /// 파라미터 클리어
    fn param_clear(&self, shell: &mut CypherShell) -> Result<CommandResult, ShellError> {
        shell.parameters_mut().clear();
        Ok(CommandResult::Success(Some(
            "All parameters cleared".to_string(),
        )))
    }

    /// 출력 포맷 변경
    fn format(&self, shell: &mut CypherShell, format: &str) -> Result<CommandResult, ShellError> {
        self.set(shell, "format", format)
    }

    /// 파일 실행
    fn source(&self, shell: &mut CypherShell, path: &str) -> Result<CommandResult, ShellError> {
        let path = PathBuf::from(path);
        let outputs = shell.execute_file(&path)?;

        let mut result = format!(
            "Executed {} command(s) from {}\n",
            outputs.len(),
            path.display()
        );
        for output in outputs {
            result.push_str(&output);
            result.push('\n');
        }

        Ok(CommandResult::Success(Some(result)))
    }

    /// 연결
    fn connect(
        &self,
        shell: &mut CypherShell,
        addr: Option<&str>,
    ) -> Result<CommandResult, ShellError> {
        if let Some(addr) = addr {
            // bolt:// URL 파싱
            let parsed = parse_bolt_url(addr)?;
            shell.config_mut().address = format!("{}:{}", parsed.host, parsed.port);
            shell.config_mut().database = parsed.database.clone();
            shell.config_mut().username = parsed.username.clone();

            // 원격 연결인 경우
            if addr.starts_with("bolt://") || addr.starts_with("bolt+s://") {
                shell.connect_remote(&parsed.username, &parsed.password)?;
            } else {
                shell.connect()?;
            }
        } else {
            shell.connect()?;
        }
        Ok(CommandResult::Success(Some(format!(
            "Connected to {}",
            shell.config().address
        ))))
    }

    /// 연결 해제
    fn disconnect(&self, shell: &mut CypherShell) -> Result<CommandResult, ShellError> {
        shell.disconnect();
        Ok(CommandResult::Success(Some("Disconnected".to_string())))
    }

    /// 데이터베이스 목록
    fn databases(&self, shell: &mut CypherShell) -> Result<CommandResult, ShellError> {
        if !shell.is_connected() {
            return Err(ShellError::ConnectionError("Not connected".to_string()));
        }

        let params = serde_json::Map::new();

        // SHOW DATABASES 쿼리 실행
        match shell.run_internal_query("SHOW DATABASES", &params) {
            Ok(result) => {
                let mut output = String::from("Databases:\n");

                if result.rows.is_empty() {
                    output.push_str("  (no databases found)\n");
                } else {
                    // 컬럼 인덱스 찾기
                    let name_idx = result.columns.iter().position(|c| c == "name").unwrap_or(0);
                    let default_idx = result.columns.iter().position(|c| c == "default");
                    let status_idx = result.columns.iter().position(|c| c == "currentStatus");

                    for row in &result.rows {
                        let name = row
                            .values
                            .get(name_idx)
                            .and_then(|v: &serde_json::Value| v.as_str())
                            .unwrap_or("unknown");

                        let is_default = default_idx
                            .and_then(|idx| row.values.get(idx))
                            .and_then(|v: &serde_json::Value| v.as_bool())
                            .unwrap_or(false);

                        let status = status_idx
                            .and_then(|idx| row.values.get(idx))
                            .and_then(|v: &serde_json::Value| v.as_str())
                            .unwrap_or("");

                        let mut db_info = format!("  - {}", name);
                        if is_default {
                            db_info.push_str(" (default)");
                        }
                        if !status.is_empty() && status != "online" {
                            db_info.push_str(&format!(" [{}]", status));
                        }
                        db_info.push('\n');
                        output.push_str(&db_info);
                    }
                }

                // 현재 데이터베이스 표시
                output.push_str(&format!(
                    "\nCurrent database: {}\n",
                    shell.config().database
                ));

                Ok(CommandResult::Success(Some(output)))
            }
            Err(error) => Err(error),
        }
    }

    fn begin(&self, shell: &mut CypherShell) -> Result<CommandResult, ShellError> {
        shell.begin_transaction()?;
        Ok(CommandResult::Success(Some(
            "Transaction started".to_string(),
        )))
    }

    fn commit(&self, shell: &mut CypherShell) -> Result<CommandResult, ShellError> {
        let info = shell.commit_transaction()?;
        Ok(CommandResult::Success(Some(format!(
            "Transaction committed ({} queries, {:.2?})",
            info.query_count, info.duration
        ))))
    }

    fn rollback(&self, shell: &mut CypherShell) -> Result<CommandResult, ShellError> {
        let info = shell.rollback_transaction()?;
        Ok(CommandResult::Success(Some(format!(
            "Transaction rolled back ({} queries, {:.2?})",
            info.query_count, info.duration
        ))))
    }

    fn tx_status(&self, shell: &CypherShell) -> Result<CommandResult, ShellError> {
        Ok(CommandResult::Success(Some(shell.transaction_status())))
    }

    /// 쿼리 타임아웃 설정/조회
    fn timeout(
        &self,
        shell: &mut CypherShell,
        seconds: Option<u64>,
    ) -> Result<CommandResult, ShellError> {
        match seconds {
            Some(secs) => {
                // 타임아웃 설정
                let timeout = if secs == 0 {
                    None
                } else {
                    Some(std::time::Duration::from_secs(secs))
                };
                shell.config_mut().query_timeout = timeout;

                let msg = if secs == 0 {
                    "Query timeout disabled (unlimited)".to_string()
                } else {
                    format!("Query timeout set to {} seconds", secs)
                };
                Ok(CommandResult::Success(Some(msg)))
            }
            None => {
                // 현재 타임아웃 조회
                let timeout_str = match shell.config().query_timeout {
                    Some(d) => format!("{} seconds", d.as_secs()),
                    None => "unlimited".to_string(),
                };
                Ok(CommandResult::Success(Some(format!(
                    "Query timeout: {}",
                    timeout_str
                ))))
            }
        }
    }

    /// EXPLAIN 쿼리 실행 계획 표시
    fn explain(&self, shell: &mut CypherShell, query: &str) -> Result<CommandResult, ShellError> {
        if !shell.is_connected() {
            return Err(ShellError::ConnectionError("Not connected".to_string()));
        }

        // EXPLAIN 접두사 추가
        let explain_query = format!("EXPLAIN {}", query);
        let params = serde_json::Map::new();

        match shell.run_internal_query(&explain_query, &params) {
            Ok(result) => {
                let mut output = String::from("Query Execution Plan:\n");
                output.push_str("═══════════════════════════════════════\n\n");

                if result.rows.is_empty() {
                    output.push_str("  (No plan data returned)\n");
                } else {
                    // Plan 정보 출력
                    for (i, row) in result.rows.iter().enumerate() {
                        output.push_str(&format!("Step {}:\n", i + 1));
                        for (col_idx, col_name) in result.columns.iter().enumerate() {
                            if let Some(value) = row.values.get(col_idx) {
                                output.push_str(&format!("  {}: {}\n", col_name, value));
                            }
                        }
                        output.push('\n');
                    }
                }

                output.push_str("═══════════════════════════════════════\n");
                output.push_str(&format!("Query: {}\n", query));

                Ok(CommandResult::Success(Some(output)))
            }
            Err(e) => {
                // EXPLAIN이 지원되지 않는 경우
                let output = format!(
                    "Query Plan:\n  Could not generate execution plan.\n  Error: {}\n\n\
                    Note: EXPLAIN requires server support. Use ':profile' for actual execution statistics.",
                    e
                );
                Ok(CommandResult::Success(Some(output)))
            }
        }
    }

    /// PROFILE 쿼리 실행 (실제 통계 포함)
    fn profile(&self, shell: &mut CypherShell, query: &str) -> Result<CommandResult, ShellError> {
        if !shell.is_connected() {
            return Err(ShellError::ConnectionError("Not connected".to_string()));
        }

        // PROFILE 접두사 추가
        let profile_query = format!("PROFILE {}", query);
        let params = serde_json::Map::new();

        let start_time = std::time::Instant::now();
        match shell.run_internal_query(&profile_query, &params) {
            Ok(result) => {
                let elapsed = start_time.elapsed();

                let mut output = String::from("Query Profile (Execution Statistics):\n");
                output.push_str("═══════════════════════════════════════\n\n");

                // 실행 시간
                output.push_str(&format!(
                    "Execution Time: {:.3}ms\n",
                    elapsed.as_secs_f64() * 1000.0
                ));
                output.push_str(&format!("Rows Returned: {}\n\n", result.rows.len()));

                if !result.rows.is_empty() {
                    // Profile 정보 출력
                    output.push_str("Execution Steps:\n");
                    for (i, row) in result.rows.iter().enumerate() {
                        output.push_str(&format!("  Step {}:\n", i + 1));
                        for (col_idx, col_name) in result.columns.iter().enumerate() {
                            if let Some(value) = row.values.get(col_idx) {
                                output.push_str(&format!("    {}: {}\n", col_name, value));
                            }
                        }
                    }
                    output.push('\n');
                }

                output.push_str("═══════════════════════════════════════\n");
                output.push_str(&format!("Query: {}\n", query));

                Ok(CommandResult::Success(Some(output)))
            }
            Err(e) => {
                let elapsed = start_time.elapsed();

                // PROFILE이 지원되지 않는 경우 일반 실행 시도
                let output = format!(
                    "Query Profile:\n\
                    ═══════════════════════════════════════\n\n\
                    Execution Time: {:.3}ms\n\
                    Status: Failed\n\
                    Error: {}\n\n\
                    Note: PROFILE requires server support.\n\
                    ═══════════════════════════════════════\n\
                    Query: {}\n",
                    elapsed.as_secs_f64() * 1000.0,
                    e,
                    query
                );
                Ok(CommandResult::Success(Some(output)))
            }
        }
    }

    /// 페이지 크기 설정/조회
    fn page(
        &self,
        shell: &mut CypherShell,
        size: Option<usize>,
    ) -> Result<CommandResult, ShellError> {
        match size {
            Some(s) => {
                // 페이지 크기 설정
                shell.config_mut().page_size = if s == 0 { None } else { Some(s) };
                shell.reset_page();

                let msg = if s == 0 {
                    "Page size: unlimited (all results shown)".to_string()
                } else {
                    format!("Page size set to {} rows", s)
                };
                Ok(CommandResult::Success(Some(msg)))
            }
            None => {
                // 현재 페이지 크기 조회
                let size_str = match shell.config().page_size {
                    Some(s) => format!("{} rows", s),
                    None => "unlimited".to_string(),
                };
                Ok(CommandResult::Success(Some(format!(
                    "Page size: {}",
                    size_str
                ))))
            }
        }
    }

    /// 다음 페이지 표시
    fn more(&self, shell: &mut CypherShell) -> Result<CommandResult, ShellError> {
        if shell.last_result().is_none() {
            return Ok(CommandResult::Success(Some(
                "No previous query result. Run a query first.".to_string(),
            )));
        }

        if !shell.has_more_pages() {
            return Ok(CommandResult::Success(Some(
                "No more results to display.".to_string(),
            )));
        }

        shell.next_page();

        let formatter =
            crate::formatter::FormatterFactory::create(shell.config().format, shell.config().color);

        if let Some(page_result) = shell.get_page_result() {
            let mut output = formatter.format(&page_result);

            // 더 많은 결과가 있으면 알림
            if shell.has_more_pages() {
                if let Some(total) = shell.last_result().map(|r| r.rows.len()) {
                    let page_size = shell.config().page_size.unwrap_or(total);
                    let current_page = shell.current_page();
                    let start = current_page * page_size;
                    let end = std::cmp::min(start + page_size, total);
                    output.push_str(&format!(
                        "\n... Showing rows {}-{} of {}. Use :more for next page.\n",
                        start + 1,
                        end,
                        total
                    ));
                }
            } else {
                output.push_str("\n... End of results.\n");
            }

            Ok(CommandResult::Success(Some(output)))
        } else {
            Ok(CommandResult::Success(Some(
                "No more results to display.".to_string(),
            )))
        }
    }

    /// 결과 파일로 내보내기
    fn export(&self, shell: &CypherShell, path: &str) -> Result<CommandResult, ShellError> {
        let result = shell.last_result().ok_or_else(|| {
            ShellError::CommandError("No previous query result. Run a query first.".to_string())
        })?;

        // 출력 포맷 결정 (확장자 기반)
        let format = if path.ends_with(".csv") {
            OutputFormat::Csv
        } else if path.ends_with(".json") {
            OutputFormat::Json
        } else {
            shell.config().format
        };

        let formatter = crate::formatter::FormatterFactory::create(format, false);
        let content = formatter.format(result);

        // 파일 저장
        std::fs::write(path, &content)
            .map_err(|e| ShellError::CommandError(format!("Failed to write file: {}", e)))?;

        Ok(CommandResult::Success(Some(format!(
            "Exported {} rows to '{}' ({} format, {} bytes)",
            result.rows.len(),
            path,
            format,
            content.len()
        ))))
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Rust 컴파일러 버전 정보
fn rustc_version_info() -> String {
    "unknown".to_string()
}

// ============================================================================
// Bolt URL 파싱
// ============================================================================

/// 파싱된 Bolt URL
#[derive(Debug, Clone)]
pub struct BoltUrl {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
}

impl Default for BoltUrl {
    fn default() -> Self {
        Self {
            scheme: "bolt".to_string(),
            host: "localhost".to_string(),
            port: 9045,
            username: "zeta4j".to_string(),
            password: String::new(),
            database: "zeta4j".to_string(),
        }
    }
}

/// Bolt URL 파싱
///
/// 지원 형식:
/// - bolt://localhost:7687
/// - bolt+s://localhost:7687 (TLS)
/// - localhost:7687 (scheme 생략)
pub fn parse_bolt_url(url: &str) -> Result<BoltUrl, ShellError> {
    let mut result = BoltUrl::default();

    // URL 정규화
    let url = url.trim();

    // scheme 추출
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("bolt+s://") {
        result.scheme = "bolt+s".to_string();
        ("bolt+s", rest)
    } else if let Some(rest) = url.strip_prefix("bolt://") {
        ("bolt", rest)
    } else {
        // scheme 없음 - localhost:port 형태로 가정
        ("bolt", url)
    };

    result.scheme = scheme.to_string();

    if rest.contains('@') {
        return Err(ShellError::ParseError(
            "credentials are not accepted in Bolt URLs".to_string(),
        ));
    }
    let host_part = rest;

    // 데이터베이스 추출 (/database)
    let (host_port, _db_part) = if let Some(slash_pos) = host_part.find('/') {
        let hp = &host_part[..slash_pos];
        let db = &host_part[slash_pos + 1..];
        if !db.is_empty() {
            result.database = db.to_string();
        }
        (hp, Some(db))
    } else {
        (host_part, None)
    };

    // 호스트:포트 파싱
    if let Some(colon_pos) = host_port.rfind(':') {
        result.host = host_port[..colon_pos].to_string();
        let port_str = &host_port[colon_pos + 1..];
        result.port = port_str
            .parse()
            .map_err(|_| ShellError::ParseError(format!("Invalid port: {}", port_str)))?;
    } else {
        result.host = host_port.to_string();
        // 기본 포트 7687
    }

    if result.host.is_empty() {
        return Err(ShellError::ParseError("Empty host".to_string()));
    }

    Ok(result)
}
