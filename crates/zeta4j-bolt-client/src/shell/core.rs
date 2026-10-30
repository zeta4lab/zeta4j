//! M8.1: Cypher Shell
//!
//! 대화형 Cypher 셸 및 파일 실행

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;
use zeroize::Zeroize;

use super::commands::{CommandRegistry, CommandResult, MetaCommand};
use super::parser::{ParsedCommand, ShellParser};
use super::transaction::{
    TransactionCommitInfo, TransactionManager as ShellTransactionManager, TransactionRollbackInfo,
};
use crate::bolt_client::BoltClient;
use crate::formatter::{FormatterFactory, OutputFormat};

// ============================================================================
// ShellConfig - 셸 설정
// ============================================================================

/// 셸 설정
#[derive(Debug, Clone)]
pub struct ShellConfig {
    /// 출력 포맷
    pub format: OutputFormat,
    /// 컬러 출력 여부
    pub color: bool,
    /// 페이지 크기 (None = 무제한)
    pub page_size: Option<usize>,
    /// 현재 데이터베이스
    pub database: String,
    /// 연결 주소
    pub address: String,
    /// 사용자명
    pub username: String,
    /// 프롬프트 문자열
    pub prompt: String,
    /// 히스토리 파일 경로
    pub history_file: Option<PathBuf>,
    /// 히스토리 최대 크기
    pub history_size: usize,
    /// 쿼리 타임아웃 (None = 무제한)
    pub query_timeout: Option<Duration>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            format: OutputFormat::Table,
            color: true,
            page_size: None,
            database: "zeta4j".to_string(),
            address: "127.0.0.1:9045".to_string(),
            username: "zeta4j".to_string(),
            prompt: "zeta4j> ".to_string(),
            history_file: None,
            history_size: 1000,
            query_timeout: Some(Duration::from_secs(30)), // 기본 30초
        }
    }
}

impl ShellConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 데이터베이스 설정
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = database.into();
        self
    }

    /// 주소 설정
    pub fn with_address(mut self, address: impl Into<String>) -> Self {
        self.address = address.into();
        self
    }

    /// 사용자명 설정
    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = username.into();
        self
    }

    /// 출력 포맷 설정
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.format = format;
        self
    }

    /// 컬러 설정
    pub fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    /// 프롬프트 업데이트
    pub fn update_prompt(&mut self) {
        self.prompt = format!("{}@{}> ", self.username, self.database);
    }

    /// 쿼리 타임아웃 설정
    pub fn with_query_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.query_timeout = timeout;
        self
    }

    /// 쿼리 타임아웃 설정 (초 단위)
    pub fn with_query_timeout_secs(mut self, seconds: u64) -> Self {
        self.query_timeout = if seconds == 0 {
            None
        } else {
            Some(Duration::from_secs(seconds))
        };
        self
    }
}

// ============================================================================
// ParameterStore - 파라미터 저장소
// ============================================================================

/// 쿼리 파라미터 저장소
#[derive(Debug, Clone, Default)]
pub struct ParameterStore {
    params: HashMap<String, ParamValue>,
}

/// 파라미터 값
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<ParamValue>),
    Map(HashMap<String, ParamValue>),
}

impl ParamValue {
    /// 문자열에서 파싱
    pub fn parse(s: &str) -> Self {
        let s = s.trim();

        if s.eq_ignore_ascii_case("null") {
            return ParamValue::Null;
        }
        if s.eq_ignore_ascii_case("true") {
            return ParamValue::Bool(true);
        }
        if s.eq_ignore_ascii_case("false") {
            return ParamValue::Bool(false);
        }
        if let Ok(i) = s.parse::<i64>() {
            return ParamValue::Int(i);
        }
        if let Ok(f) = s.parse::<f64>() {
            return ParamValue::Float(f);
        }
        // 따옴표 제거
        if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
            return ParamValue::String(s[1..s.len() - 1].to_string());
        }
        ParamValue::String(s.to_string())
    }

    /// JSON 값으로 변환
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ParamValue::Null => serde_json::Value::Null,
            ParamValue::Bool(b) => serde_json::Value::Bool(*b),
            ParamValue::Int(i) => serde_json::json!(*i),
            ParamValue::Float(f) => serde_json::json!(*f),
            ParamValue::String(s) => serde_json::Value::String(s.clone()),
            ParamValue::List(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| v.to_json()).collect())
            }
            ParamValue::Map(map) => {
                let obj: serde_json::Map<String, serde_json::Value> =
                    map.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
                serde_json::Value::Object(obj)
            }
        }
    }
}

impl std::fmt::Display for ParamValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParamValue::Null => write!(f, "null"),
            ParamValue::Bool(b) => write!(f, "{}", b),
            ParamValue::Int(i) => write!(f, "{}", i),
            ParamValue::Float(fl) => write!(f, "{}", fl),
            ParamValue::String(s) => write!(f, "\"{}\"", s),
            ParamValue::List(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            ParamValue::Map(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl ParameterStore {
    /// 새 저장소 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 파라미터 설정
    pub fn set(&mut self, name: impl Into<String>, value: ParamValue) {
        self.params.insert(name.into(), value);
    }

    /// 파라미터 가져오기
    pub fn get(&self, name: &str) -> Option<&ParamValue> {
        self.params.get(name)
    }

    /// 파라미터 삭제
    pub fn remove(&mut self, name: &str) -> Option<ParamValue> {
        self.params.remove(name)
    }

    /// 모든 파라미터 가져오기
    pub fn all(&self) -> &HashMap<String, ParamValue> {
        &self.params
    }

    /// 모든 파라미터 클리어
    pub fn clear(&mut self) {
        self.params.clear();
    }

    /// JSON 맵으로 변환
    pub fn to_json_map(&self) -> serde_json::Map<String, serde_json::Value> {
        self.params
            .iter()
            .map(|(k, v)| (k.clone(), v.to_json()))
            .collect()
    }
}

// ============================================================================
// CommandHistory - 명령 히스토리
// ============================================================================

/// 명령 히스토리
#[derive(Debug, Clone)]
pub struct CommandHistory {
    entries: Vec<String>,
    max_size: usize,
    current_index: Option<usize>,
}

impl CommandHistory {
    /// 새 히스토리 생성
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_size,
            current_index: None,
        }
    }

    /// 엔트리 추가
    pub fn add(&mut self, command: impl Into<String>) {
        let cmd = command.into();
        if cmd.trim().is_empty() {
            return;
        }
        // 중복 방지
        if self.entries.last().map(|s| s.as_str()) == Some(cmd.as_str()) {
            return;
        }
        self.entries.push(cmd);
        if self.entries.len() > self.max_size {
            self.entries.remove(0);
        }
        self.current_index = None;
    }

    /// 이전 엔트리
    pub fn prev(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        let idx = match self.current_index {
            Some(i) if i > 0 => i - 1,
            Some(_) => return self.entries.first().map(|s| s.as_str()),
            None => self.entries.len() - 1,
        };
        self.current_index = Some(idx);
        self.entries.get(idx).map(|s| s.as_str())
    }

    /// 다음 엔트리
    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        let idx = match self.current_index {
            Some(i) if i < self.entries.len() - 1 => i + 1,
            _ => {
                self.current_index = None;
                return None;
            }
        };
        self.current_index = Some(idx);
        self.entries.get(idx).map(|s| s.as_str())
    }

    /// 모든 엔트리
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// 엔트리 수
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 인덱스 리셋
    pub fn reset_index(&mut self) {
        self.current_index = None;
    }

    /// 파일에서 로드
    pub fn load_from_file(&mut self, path: &PathBuf) -> io::Result<()> {
        let content = fs::read_to_string(path)?;
        for line in content.lines() {
            if !line.trim().is_empty() {
                self.entries.push(line.to_string());
            }
        }
        // 최대 크기 유지
        while self.entries.len() > self.max_size {
            self.entries.remove(0);
        }
        Ok(())
    }

    /// 파일에 저장
    pub fn save_to_file(&self, path: &PathBuf) -> io::Result<()> {
        let content = self.entries.join("\n");
        fs::write(path, content)
    }
}

// ============================================================================
// QueryResult - 쿼리 결과
// ============================================================================

/// 쿼리 결과 행
#[derive(Debug, Clone)]
pub struct ResultRow {
    pub values: Vec<serde_json::Value>,
}

/// 쿼리 결과
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<ResultRow>,
    pub stats: QueryStats,
}

/// 쿼리 통계
#[derive(Debug, Clone, Default)]
pub struct QueryStats {
    pub nodes_created: u64,
    pub nodes_deleted: u64,
    pub relationships_created: u64,
    pub relationships_deleted: u64,
    pub properties_set: u64,
    pub labels_added: u64,
    pub labels_removed: u64,
    pub execution_time_ms: u64,
}

impl QueryResult {
    /// 새 결과 생성
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
            stats: QueryStats::default(),
        }
    }

    /// 행 추가
    pub fn add_row(&mut self, values: Vec<serde_json::Value>) {
        self.rows.push(ResultRow { values });
    }

    /// 행 수
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 빈 결과인지
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ============================================================================
// ShellConnection - 셸 연결
// ============================================================================

/// 연결 모드
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionMode {
    /// 로컬 임베디드 모드 (Mock)
    Local,
    /// 원격 Bolt 연결
    Remote,
}

/// 셸 연결 (서버 연결 추상화)
pub struct ShellConnection {
    address: String,
    database: String,
    username: String,
    password: String,
    bearer_token: String,
    mode: ConnectionMode,
    bolt_client: Option<BoltClient>,
}

impl ShellConnection {
    /// 새 로컬 연결 생성
    pub fn new(address: &str, database: &str) -> Self {
        Self {
            address: address.to_string(),
            database: database.to_string(),
            username: "zeta4j".to_string(),
            password: String::new(),
            bearer_token: String::new(),
            mode: ConnectionMode::Local,
            bolt_client: None,
        }
    }

    /// 새 원격 연결 생성
    pub fn new_remote(address: &str, database: &str, username: &str, password: &str) -> Self {
        Self {
            address: address.to_string(),
            database: database.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            bearer_token: String::new(),
            mode: ConnectionMode::Remote,
            bolt_client: None,
        }
    }

    pub fn new_remote_bearer(address: &str, database: &str, access_token: &str) -> Self {
        Self {
            address: address.to_string(),
            database: database.to_string(),
            username: String::new(),
            password: String::new(),
            bearer_token: access_token.to_string(),
            mode: ConnectionMode::Remote,
            bolt_client: None,
        }
    }

    /// 연결
    pub fn connect(&mut self) -> Result<(), ShellError> {
        match self.mode {
            ConnectionMode::Local => {
                // 로컬 모드는 항상 성공 (임베디드)
                Ok(())
            }
            ConnectionMode::Remote => {
                // Bolt 클라이언트로 연결
                let mut client = BoltClient::new().with_database(&self.database);

                client
                    .connect(&self.address)
                    .map_err(|e| ShellError::ConnectionError(e.to_string()))?;

                let authentication = if self.bearer_token.is_empty() {
                    client.authenticate(&self.username, &self.password)
                } else {
                    client.authenticate_bearer(&self.bearer_token)
                };
                self.password.zeroize();
                self.bearer_token.zeroize();
                authentication.map_err(|e| ShellError::ConnectionError(e.to_string()))?;

                self.bolt_client = Some(client);
                Ok(())
            }
        }
    }

    /// 연결 해제
    pub fn disconnect(&mut self) {
        self.password.zeroize();
        self.bearer_token.zeroize();
        if let Some(ref mut client) = self.bolt_client {
            let _ = client.close();
        }
        self.bolt_client = None;
    }

    /// 연결 상태
    pub fn is_connected(&self) -> bool {
        match self.mode {
            ConnectionMode::Local => true,
            ConnectionMode::Remote => self
                .bolt_client
                .as_ref()
                .map(|c| c.is_connected())
                .unwrap_or(false),
        }
    }

    /// 연결 모드
    pub fn mode(&self) -> &ConnectionMode {
        &self.mode
    }

    /// 데이터베이스 변경
    pub fn use_database(&mut self, database: &str) -> Result<(), ShellError> {
        self.database = database.to_string();
        // 원격 연결에서는 재연결 필요할 수 있음
        Ok(())
    }

    /// 현재 데이터베이스
    pub fn current_database(&self) -> &str {
        &self.database
    }

    /// 쿼리 실행
    pub fn execute_query(
        &mut self,
        query: &str,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<QueryResult, ShellError> {
        use std::time::Instant;

        let start = Instant::now();

        match self.mode {
            ConnectionMode::Local => {
                // Mock 결과 반환 (임베디드 엔진 연동 후 교체)
                let mut result = QueryResult::new(vec!["result".to_string()]);
                result.add_row(vec![serde_json::json!("Query executed (local mock)")]);
                result.stats.execution_time_ms = start.elapsed().as_millis() as u64;
                Ok(result)
            }
            ConnectionMode::Remote => {
                let client = self
                    .bolt_client
                    .as_mut()
                    .ok_or_else(|| ShellError::ConnectionError("Not connected".to_string()))?;

                // JSON 파라미터를 PackStreamValue로 변환
                let bolt_params = params
                    .iter()
                    .map(|(k, v)| (k.clone(), json_to_packstream(v)))
                    .collect();

                // 쿼리 실행
                let bolt_result = client
                    .execute(query, bolt_params)
                    .map_err(|e| ShellError::QueryError(e.to_string()))?;

                // 결과 변환
                let mut result = QueryResult::new(bolt_result.columns);
                for record in bolt_result.records {
                    let row: Vec<serde_json::Value> =
                        record.into_iter().map(|v| packstream_to_json(&v)).collect();
                    result.add_row(row);
                }

                // Bolt stats를 Shell stats로 변환
                if let Some(stats_map) = &bolt_result.stats {
                    use zeta4j_bolt_protocol::packstream::PackStreamValue;
                    let get_i64 = |key: &str| -> u64 {
                        stats_map
                            .get(key)
                            .and_then(|v| {
                                if let PackStreamValue::Integer(i) = v {
                                    Some(*i as u64)
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0)
                    };

                    result.stats.nodes_created = get_i64("nodes-created");
                    result.stats.nodes_deleted = get_i64("nodes-deleted");
                    result.stats.relationships_created = get_i64("relationships-created");
                    result.stats.relationships_deleted = get_i64("relationships-deleted");
                    result.stats.properties_set = get_i64("properties-set");
                    result.stats.labels_added = get_i64("labels-added");
                    result.stats.labels_removed = get_i64("labels-removed");
                }
                result.stats.execution_time_ms = start.elapsed().as_millis() as u64;

                Ok(result)
            }
        }
    }

    /// Start an explicit Bolt transaction.
    pub fn begin_transaction(&mut self) -> Result<(), ShellError> {
        self.remote_client_mut()?
            .begin()
            .map_err(|error| ShellError::QueryError(error.to_string()))
    }

    /// Commit the active Bolt transaction.
    pub fn commit_transaction(&mut self) -> Result<Option<String>, ShellError> {
        self.remote_client_mut()?
            .commit()
            .map_err(|error| ShellError::QueryError(error.to_string()))
    }

    /// Roll back the active Bolt transaction.
    pub fn rollback_transaction(&mut self) -> Result<(), ShellError> {
        self.remote_client_mut()?
            .rollback()
            .map_err(|error| ShellError::QueryError(error.to_string()))
    }

    /// Reset a failed Bolt session.
    pub fn reset(&mut self) -> Result<(), ShellError> {
        self.remote_client_mut()?
            .reset()
            .map_err(|error| ShellError::QueryError(error.to_string()))
    }

    fn remote_client_mut(&mut self) -> Result<&mut BoltClient, ShellError> {
        if self.mode != ConnectionMode::Remote {
            return Err(ShellError::CommandError(
                "Explicit transactions require a remote Bolt connection".to_string(),
            ));
        }
        self.bolt_client
            .as_mut()
            .ok_or_else(|| ShellError::ConnectionError("Not connected".to_string()))
    }
}

/// JSON 값을 PackStreamValue로 변환
fn json_to_packstream(
    value: &serde_json::Value,
) -> zeta4j_bolt_protocol::packstream::PackStreamValue {
    use zeta4j_bolt_protocol::packstream::PackStreamValue;

    match value {
        serde_json::Value::Null => PackStreamValue::Null,
        serde_json::Value::Bool(b) => PackStreamValue::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                PackStreamValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                PackStreamValue::Float(f)
            } else {
                PackStreamValue::Null
            }
        }
        serde_json::Value::String(s) => PackStreamValue::String(s.clone()),
        serde_json::Value::Array(arr) => {
            PackStreamValue::List(arr.iter().map(json_to_packstream).collect())
        }
        serde_json::Value::Object(map) => PackStreamValue::Map(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_packstream(v)))
                .collect(),
        ),
    }
}

/// PackStreamValue를 JSON 값으로 변환
fn packstream_to_json(
    value: &zeta4j_bolt_protocol::packstream::PackStreamValue,
) -> serde_json::Value {
    use zeta4j_bolt_protocol::packstream::PackStreamValue;

    match value {
        PackStreamValue::Null => serde_json::Value::Null,
        PackStreamValue::Boolean(b) => serde_json::Value::Bool(*b),
        PackStreamValue::Integer(i) => serde_json::json!(*i),
        PackStreamValue::Float(f) => serde_json::json!(*f),
        PackStreamValue::String(s) => serde_json::Value::String(s.clone()),
        PackStreamValue::Bytes(b) => {
            // Base64 인코딩
            use base64::{Engine, engine::general_purpose::STANDARD};
            serde_json::Value::String(STANDARD.encode(b))
        }
        PackStreamValue::List(arr) => {
            serde_json::Value::Array(arr.iter().map(packstream_to_json).collect())
        }
        PackStreamValue::Map(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), packstream_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        PackStreamValue::Structure(s) => {
            // 구조체를 맵으로 변환
            let mut obj = serde_json::Map::new();
            obj.insert(
                "_type".to_string(),
                serde_json::json!(format!("0x{:02X}", s.tag)),
            );
            obj.insert(
                "_fields".to_string(),
                serde_json::Value::Array(s.fields.iter().map(packstream_to_json).collect()),
            );
            serde_json::Value::Object(obj)
        }
    }
}

// ============================================================================
// ShellError - 에러 타입
// ============================================================================

/// 셸 에러
#[derive(Debug, Clone)]
pub enum ShellError {
    /// 연결 에러
    ConnectionError(String),
    /// 쿼리 에러
    QueryError(String),
    /// 파싱 에러
    ParseError(String),
    /// IO 에러
    IoError(String),
    /// 명령 에러
    CommandError(String),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            ShellError::QueryError(msg) => write!(f, "Query error: {}", msg),
            ShellError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            ShellError::IoError(msg) => write!(f, "IO error: {}", msg),
            ShellError::CommandError(msg) => write!(f, "Command error: {}", msg),
        }
    }
}

impl std::error::Error for ShellError {}

impl From<io::Error> for ShellError {
    fn from(e: io::Error) -> Self {
        ShellError::IoError(e.to_string())
    }
}

// ============================================================================
// CypherShell - 대화형 셸
// ============================================================================

/// Cypher 대화형 셸
#[expect(dead_code)]
pub struct CypherShell {
    /// 설정
    config: ShellConfig,
    /// 연결
    connection: Option<ShellConnection>,
    /// 파라미터
    parameters: ParameterStore,
    /// 히스토리
    history: CommandHistory,
    /// 파서
    parser: ShellParser,
    /// 명령 레지스트리
    commands: CommandRegistry,
    /// 현재 입력 버퍼 (멀티라인)
    input_buffer: String,
    /// 실행 중 여부
    running: bool,
    /// 마지막 쿼리 결과 (페이징용)
    last_result: Option<QueryResult>,
    /// 현재 페이지 위치
    current_page: usize,
    /// 명시적 Bolt 트랜잭션 상태
    transaction: ShellTransactionManager,
}

impl CypherShell {
    /// 새 셸 생성
    pub fn new(config: ShellConfig) -> Self {
        let history_size = config.history_size;
        Self {
            config,
            connection: None,
            parameters: ParameterStore::new(),
            history: CommandHistory::new(history_size),
            parser: ShellParser::new(),
            commands: CommandRegistry::new(),
            input_buffer: String::new(),
            running: false,
            last_result: None,
            current_page: 0,
            transaction: ShellTransactionManager::new(),
        }
    }

    /// 연결 (로컬 모드)
    pub fn connect(&mut self) -> Result<(), ShellError> {
        let mut conn = ShellConnection::new(&self.config.address, &self.config.database);
        conn.connect()?;
        self.connection = Some(conn);
        Ok(())
    }

    /// 원격 연결 (Bolt 프로토콜)
    pub fn connect_remote(&mut self, username: &str, password: &str) -> Result<(), ShellError> {
        let mut conn = ShellConnection::new_remote(
            &self.config.address,
            &self.config.database,
            username,
            password,
        );
        conn.connect()?;
        self.connection = Some(conn);
        Ok(())
    }

    pub fn connect_remote_bearer(&mut self, access_token: &str) -> Result<(), ShellError> {
        let mut conn = ShellConnection::new_remote_bearer(
            &self.config.address,
            &self.config.database,
            access_token,
        );
        conn.connect()?;
        self.connection = Some(conn);
        Ok(())
    }

    /// 연결 해제
    pub fn disconnect(&mut self) {
        if let Some(ref mut conn) = self.connection {
            if self.transaction.state().is_active() {
                let _ = conn.rollback_transaction();
            } else if self.transaction.state().is_failed() {
                let _ = conn.reset();
            }
            conn.disconnect();
        }
        if !self.transaction.state().is_none() {
            let _ = self.transaction.rollback();
        }
        self.connection = None;
    }

    /// 서버가 인증 없이 접속 가능한지 확인
    ///
    /// Returns:
    /// - `Ok(true)` - 인증 불필요 (이미 연결됨)
    /// - `Ok(false)` - 인증 필요
    /// - `Err` - 연결 실패 (서버 다운, 네트워크 오류 등)
    pub fn try_connect_without_auth(&mut self) -> Result<bool, ShellError> {
        // 빈 인증 정보로 연결 시도
        match self.connect_remote("", "") {
            Ok(()) => Ok(true), // 인증 없이 연결 성공
            Err(e) => {
                let msg = e.to_string();
                // Unauthorized 에러면 인증 필요 (연결은 실패했으므로 재시도 가능)
                if msg.contains("Unauthorized")
                    || msg.contains("Zeta.ClientError.Security")
                    || msg.contains("Authentication")
                {
                    Ok(false)
                } else {
                    Err(e) // 다른 에러 (서버 다운, 네트워크 오류 등)
                }
            }
        }
    }

    /// 연결 상태
    pub fn is_connected(&self) -> bool {
        self.connection
            .as_ref()
            .map(|c| c.is_connected())
            .unwrap_or(false)
    }

    /// 설정 가져오기
    pub fn config(&self) -> &ShellConfig {
        &self.config
    }

    /// 설정 변경
    pub fn config_mut(&mut self) -> &mut ShellConfig {
        &mut self.config
    }

    /// 파라미터 저장소
    pub fn parameters(&self) -> &ParameterStore {
        &self.parameters
    }

    /// 파라미터 저장소 (mutable)
    pub fn parameters_mut(&mut self) -> &mut ParameterStore {
        &mut self.parameters
    }

    /// 히스토리
    pub fn history(&self) -> &CommandHistory {
        &self.history
    }

    /// 마지막 결과 저장
    pub fn set_last_result(&mut self, result: QueryResult) {
        self.last_result = Some(result);
        self.current_page = 0;
    }

    /// 마지막 결과 조회
    pub fn last_result(&self) -> Option<&QueryResult> {
        self.last_result.as_ref()
    }

    /// 현재 페이지
    pub fn current_page(&self) -> usize {
        self.current_page
    }

    /// 다음 페이지로 이동
    pub fn next_page(&mut self) {
        self.current_page += 1;
    }

    /// 페이지 초기화
    pub fn reset_page(&mut self) {
        self.current_page = 0;
    }

    /// 특정 페이지의 결과 가져오기
    pub fn get_page_result(&self) -> Option<QueryResult> {
        let result = self.last_result.as_ref()?;
        // Update-only statements have no rows but still carry mutation stats.
        // They must reach the formatter even when a configured page size is
        // nonzero, otherwise the shell replaces the stats with "No results".
        if result.rows.is_empty() {
            return Some(result.clone());
        }
        let page_size = self.config.page_size.unwrap_or(result.rows.len());

        if page_size == 0 {
            return Some(result.clone());
        }

        let start = self.current_page * page_size;
        if start >= result.rows.len() {
            return None;
        }

        let end = std::cmp::min(start + page_size, result.rows.len());
        let page_rows = result.rows[start..end].to_vec();

        Some(QueryResult {
            columns: result.columns.clone(),
            rows: page_rows,
            stats: result.stats.clone(),
        })
    }

    /// 더 많은 결과가 있는지 확인
    pub fn has_more_pages(&self) -> bool {
        if let Some(result) = &self.last_result {
            let page_size = self.config.page_size.unwrap_or(result.rows.len());
            if page_size == 0 {
                return false;
            }
            let start = (self.current_page + 1) * page_size;
            start < result.rows.len()
        } else {
            false
        }
    }

    /// 내부 쿼리 실행 (히스토리에 추가하지 않음)
    ///
    /// 메타 명령에서 스키마 조회 등에 사용됩니다.
    pub fn run_internal_query(
        &mut self,
        query: &str,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<QueryResult, ShellError> {
        let conn = self
            .connection
            .as_mut()
            .ok_or_else(|| ShellError::ConnectionError("Not connected".to_string()))?;
        conn.execute_query(query, params)
    }

    /// 명령 실행
    pub fn execute(&mut self, input: &str) -> Result<Option<String>, ShellError> {
        let parsed = self.parser.parse(input)?;

        match parsed {
            ParsedCommand::Empty => Ok(None),
            ParsedCommand::Query(query) => self.execute_query(&query),
            ParsedCommand::Meta(cmd) => self.execute_meta_command(cmd),
            ParsedCommand::Exit => {
                self.running = false;
                Ok(Some("Bye!".to_string()))
            }
        }
    }

    /// 쿼리 실행
    fn execute_query(&mut self, query: &str) -> Result<Option<String>, ShellError> {
        self.history.add(query);

        let params = self.parameters.to_json_map();

        if self.transaction.state().is_failed() {
            return Err(ShellError::CommandError(
                "Transaction is failed; use :rollback to reset it".to_string(),
            ));
        }

        let conn = self
            .connection
            .as_mut()
            .ok_or_else(|| ShellError::ConnectionError("Not connected".to_string()))?;

        let result = match conn.execute_query(query, &params) {
            Ok(result) => {
                self.transaction.increment_query_count();
                result
            }
            Err(error) => {
                if self.transaction.state().is_active() {
                    self.transaction.mark_failed(error.to_string());
                }
                return Err(error);
            }
        };

        // 결과 저장 (페이징용)
        let total_rows = result.rows.len();
        self.set_last_result(result);

        // 페이징 적용
        let formatter = FormatterFactory::create(self.config.format, self.config.color);
        let mut output = if let Some(page_result) = self.get_page_result() {
            formatter.format(&page_result)
        } else {
            "No results".to_string()
        };

        // 더 많은 결과가 있으면 알림
        if self.has_more_pages() {
            let page_size = self.config.page_size.unwrap_or(total_rows);
            let shown = std::cmp::min(page_size, total_rows);
            output.push_str(&format!(
                "\n... Showing {} of {} rows. Use :more for next page, :page 0 for unlimited.\n",
                shown, total_rows
            ));
        }

        Ok(Some(output))
    }

    /// 메타 명령 실행
    fn execute_meta_command(&mut self, cmd: MetaCommand) -> Result<Option<String>, ShellError> {
        // CommandRegistry는 상태가 없으므로 새 인스턴스를 생성하여 borrow 충돌 방지
        let registry = CommandRegistry::new();
        let result = registry.execute(cmd, self)?;
        match result {
            CommandResult::Success(msg) => Ok(msg),
            CommandResult::Error(msg) => Err(ShellError::CommandError(msg)),
            CommandResult::Exit => {
                self.running = false;
                Ok(Some("Bye!".to_string()))
            }
        }
    }

    /// 파일에서 실행
    pub fn execute_file(&mut self, path: &PathBuf) -> Result<Vec<String>, ShellError> {
        let content = fs::read_to_string(path)?;
        let mut outputs = Vec::new();

        for line in content.lines() {
            if let Some(output) = self.execute(line)? {
                outputs.push(output);
            }
        }

        // File mode accepts a final statement without a semicolon. Interactive
        // mode keeps waiting, but EOF is an unambiguous statement boundary.
        if self.parser.is_multiline()
            && let Some(output) = self.execute(";")?
        {
            outputs.push(output);
        }

        Ok(outputs)
    }

    /// 대화형 모드 실행
    pub fn run_interactive(
        &mut self,
        input: &mut dyn BufRead,
        output: &mut dyn Write,
    ) -> Result<(), ShellError> {
        self.running = true;

        // 히스토리 로드
        if let Some(ref path) = self.config.history_file {
            let _ = self.history.load_from_file(path);
        }

        writeln!(output, "Connected to Zeta4j at {}", self.config.address)?;
        writeln!(output, "Type :help for more information.")?;

        while self.running {
            // 프롬프트 출력
            write!(output, "{}", self.config.prompt)?;
            output.flush()?;

            // 입력 읽기
            let mut line = String::new();
            match input.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {}
                Err(e) => return Err(ShellError::IoError(e.to_string())),
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // 명령 실행
            match self.execute(line) {
                Ok(Some(result)) => {
                    writeln!(output, "{}", result)?;
                }
                Ok(None) => {}
                Err(e) => {
                    writeln!(output, "ERROR: {}", e)?;
                }
            }
        }

        // 히스토리 저장
        if let Some(ref path) = self.config.history_file {
            let _ = self.history.save_to_file(path);
        }

        Ok(())
    }

    /// 데이터베이스 변경
    pub fn use_database(&mut self, database: &str) -> Result<(), ShellError> {
        if let Some(ref mut conn) = self.connection {
            conn.use_database(database)?;
        }
        self.config.database = database.to_string();
        self.config.update_prompt();
        Ok(())
    }

    /// Begin an explicit transaction on the active Bolt connection.
    pub fn begin_transaction(&mut self) -> Result<(), ShellError> {
        if !self.transaction.state().is_none() {
            return Err(ShellError::CommandError(self.transaction.status_string()));
        }
        let conn = self
            .connection
            .as_mut()
            .ok_or_else(|| ShellError::ConnectionError("Not connected".to_string()))?;
        conn.begin_transaction()?;
        self.transaction
            .begin("bolt".to_string())
            .map_err(|error| ShellError::CommandError(error.to_string()))
    }

    /// Commit the current explicit transaction.
    pub fn commit_transaction(&mut self) -> Result<TransactionCommitInfo, ShellError> {
        if self.transaction.state().is_none() {
            return Err(ShellError::CommandError(
                "No active transaction".to_string(),
            ));
        }
        if self.transaction.state().is_failed() {
            return Err(ShellError::CommandError(
                "Transaction is failed; use :rollback to reset it".to_string(),
            ));
        }
        let conn = self
            .connection
            .as_mut()
            .ok_or_else(|| ShellError::ConnectionError("Not connected".to_string()))?;
        let bookmark = conn.commit_transaction()?;
        self.transaction
            .commit(bookmark)
            .map_err(|error| ShellError::CommandError(error.to_string()))
    }

    /// Roll back an active transaction or reset a failed Bolt session.
    pub fn rollback_transaction(&mut self) -> Result<TransactionRollbackInfo, ShellError> {
        if self.transaction.state().is_none() {
            return Err(ShellError::CommandError(
                "No active transaction".to_string(),
            ));
        }
        let failed = self.transaction.state().is_failed();
        let conn = self
            .connection
            .as_mut()
            .ok_or_else(|| ShellError::ConnectionError("Not connected".to_string()))?;
        if failed {
            conn.reset()?;
        } else {
            conn.rollback_transaction()?;
        }
        self.transaction
            .rollback()
            .map_err(|error| ShellError::CommandError(error.to_string()))
    }

    /// Human-readable transaction state for `:tx-status`.
    pub fn transaction_status(&self) -> String {
        self.transaction.status_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_page_preserves_mutation_stats() -> Result<(), &'static str> {
        let mut config = ShellConfig::new();
        config.page_size = Some(100);
        let mut shell = CypherShell::new(config);
        let mut result = QueryResult::new(Vec::new());
        result.stats.nodes_deleted = 1;
        shell.set_last_result(result);

        let page = shell
            .get_page_result()
            .ok_or("an update-only result must remain formatable")?;
        assert_eq!(page.stats.nodes_deleted, 1);
        assert!(page.rows.is_empty());
        Ok(())
    }
}
