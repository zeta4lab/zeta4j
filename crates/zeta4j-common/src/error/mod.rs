//! 통합 에러 타입
//!
//! 모든 zeta4j 크레이트에서 사용할 수 있는 통합 에러 타입입니다.
//!
//! # 에러 계층 구조
//!
//! ```text
//! ZetaError
//! ├── Core        - 그래프 엔진 관련 에러
//! ├── Storage     - 스토리지 관련 에러
//! ├── Query       - 쿼리 실행 관련 에러
//! ├── Transaction - 트랜잭션 관련 에러
//! ├── Server      - 서버 관련 에러
//! ├── Auth        - 인증/권한 관련 에러
//! ├── Config      - 설정 관련 에러
//! ├── Network     - 네트워크 관련 에러
//! ├── Vector      - 벡터 인덱스 관련 에러
//! └── Internal    - 내부 오류
//! ```
//!
//! # 에러 코드
//!
//! 모든 에러는 구조화된 코드를 가지며, [`codes`] 모듈에서 정의됩니다.
//!
//! ```rust,ignore
//! use zeta4j_common::error::codes;
//!
//! let err = ZetaError::node_not_found(42);
//! println!("Error code: {}", err.code()); // GR-NODE-001
//! ```

pub mod codes;

pub use codes::ErrorCode;

use thiserror::Error;

/// 통합 Result 타입
pub type Result<T> = std::result::Result<T, ZetaError>;

/// Zeta4j 통합 에러 타입
///
/// 모든 크레이트에서 발생할 수 있는 에러를 통합합니다.
#[derive(Error, Debug)]
pub enum ZetaError {
    // =========================================================================
    // Core Errors (GR)
    // =========================================================================
    /// 그래프 노드 관련 에러
    #[error("[{code}] Node error: {message}")]
    Node { code: ErrorCode, message: String },

    /// 노드를 찾을 수 없음
    #[error("[{code}] Node not found: {id}")]
    NodeNotFound { code: ErrorCode, id: u64 },

    /// 그래프 엣지 관련 에러
    #[error("[{code}] Edge error: {message}")]
    Edge { code: ErrorCode, message: String },

    /// 엣지를 찾을 수 없음
    #[error("[{code}] Edge not found: {id}")]
    EdgeNotFound { code: ErrorCode, id: u64 },

    /// 프로퍼티 관련 에러
    #[error("[{code}] Property error: {message}")]
    Property { code: ErrorCode, message: String },

    /// 레이블 관련 에러
    #[error("[{code}] Label error: {message}")]
    Label { code: ErrorCode, message: String },

    /// 인덱스 관련 에러
    #[error("[{code}] Index error: {message}")]
    Index { code: ErrorCode, message: String },

    /// 제약조건 관련 에러
    #[error("[{code}] Constraint violation: {constraint_name} - {message}")]
    Constraint {
        code: ErrorCode,
        constraint_name: String,
        message: String,
    },

    // =========================================================================
    // Storage Errors (GR-STOR)
    // =========================================================================
    /// 스토리지 I/O 에러
    #[error("[{code}] Storage I/O error: {message}")]
    StorageIo { code: ErrorCode, message: String },

    /// 스토리지 손상 에러
    #[error("[{code}] Storage corruption: {message}")]
    StorageCorruption { code: ErrorCode, message: String },

    /// 스토리지 용량 초과
    #[error("[{code}] Storage capacity exceeded: {message}")]
    StorageCapacity { code: ErrorCode, message: String },

    /// 직렬화 에러
    #[error("[{code}] Serialization error: {message}")]
    Serialization { code: ErrorCode, message: String },

    /// Database-scoped memory admission rejection.
    #[error("[{code}] Memory admission denied: {message}")]
    MemoryAdmission { code: ErrorCode, message: String },

    // =========================================================================
    // Transaction Errors (TX)
    // =========================================================================
    /// 일반 트랜잭션 에러
    #[error("[{code}] Transaction error: {message}")]
    Transaction { code: ErrorCode, message: String },

    /// 트랜잭션을 찾을 수 없음
    #[error("[{code}] Transaction not found: {tx_id}")]
    TransactionNotFound { code: ErrorCode, tx_id: u64 },

    /// 락 타임아웃
    #[error("[{code}] Lock timeout on {resource} after {waited_ms}ms")]
    LockTimeout {
        code: ErrorCode,
        resource: String,
        waited_ms: u64,
    },

    /// 데드락 감지
    #[error("[{code}] Deadlock detected involving transactions: {cycle:?}")]
    Deadlock { code: ErrorCode, cycle: Vec<u64> },

    /// 쓰기 충돌
    #[error("[{code}] Write conflict on key {key}: modified by transaction {conflicting_tx}")]
    WriteConflict {
        code: ErrorCode,
        key: String,
        conflicting_tx: u64,
    },

    /// WAL 손상
    #[error("[{code}] WAL corrupted at position {position}: {details}")]
    WalCorrupted {
        code: ErrorCode,
        position: u64,
        details: String,
    },

    /// 락 획득 실패
    #[error("[{code}] Lock acquisition failed: {message}")]
    LockFailed { code: ErrorCode, message: String },

    // =========================================================================
    // Query Errors (CY)
    // =========================================================================
    /// 쿼리 파싱 에러
    #[error("[{code}] Query parse error: {message}")]
    QueryParse { code: ErrorCode, message: String },

    /// 쿼리 실행 에러
    #[error("[{code}] Query execution error: {message}")]
    QueryExecution { code: ErrorCode, message: String },

    /// 쿼리 타임아웃
    #[error("[{code}] Query timeout: {message}")]
    QueryTimeout { code: ErrorCode, message: String },

    /// 타입 불일치
    #[error("[{code}] Type mismatch: expected {expected}, found {found}")]
    TypeMismatch {
        code: ErrorCode,
        expected: String,
        found: String,
    },

    /// 프로시저 에러
    #[error("[{code}] Procedure error: {message}")]
    Procedure { code: ErrorCode, message: String },

    // =========================================================================
    // Fulltext Errors (FT)
    // =========================================================================
    /// 전문 검색 에러
    #[error("[{code}] Fulltext error: {message}")]
    Fulltext { code: ErrorCode, message: String },

    // =========================================================================
    // Vector Errors (VE)
    // =========================================================================
    /// 벡터 인덱스 에러
    #[error("[{code}] Vector index error: {message}")]
    VectorIndex { code: ErrorCode, message: String },

    /// 차원 불일치
    #[error("[{code}] Dimension mismatch: expected {expected}, found {found}")]
    DimensionMismatch {
        code: ErrorCode,
        expected: usize,
        found: usize,
    },

    // =========================================================================
    // Cache Errors (CA)
    // =========================================================================
    /// 캐시 직렬화 에러
    #[error("[{code}] Cache serialization error: {message}")]
    CacheSerialization { code: ErrorCode, message: String },

    /// 캐시 저장소 에러
    #[error("[{code}] Cache storage error: {message}")]
    CacheStorage { code: ErrorCode, message: String },

    /// 캐시 초기화 에러
    #[error("[{code}] Cache initialization error: {message}")]
    CacheInit { code: ErrorCode, message: String },

    // =========================================================================
    // Server Errors (SV)
    // =========================================================================
    /// HTTP 서버 에러
    #[error("[{code}] HTTP error: {status} - {message}")]
    Http {
        code: ErrorCode,
        status: u16,
        message: String,
    },

    /// Bolt 프로토콜 에러
    #[error("[{code}] Bolt protocol error: {message}")]
    Bolt { code: ErrorCode, message: String },

    /// 연결 에러
    #[error("[{code}] Connection error: {message}")]
    Connection { code: ErrorCode, message: String },

    // =========================================================================
    // Auth Errors (SV-AUTH)
    // =========================================================================
    /// 인증 실패
    #[error("[{code}] Authentication failed: {message}")]
    AuthFailed { code: ErrorCode, message: String },

    /// 권한 없음
    #[error("[{code}] Permission denied: {message}")]
    PermissionDenied { code: ErrorCode, message: String },

    /// 토큰 만료
    #[error("[{code}] Token expired")]
    TokenExpired { code: ErrorCode },

    // =========================================================================
    // Config Errors (SV-CONFIG)
    // =========================================================================
    /// 설정 파싱 에러
    #[error("[{code}] Config parse error: {message}")]
    ConfigParse { code: ErrorCode, message: String },

    /// 잘못된 설정 값
    #[error("[{code}] Invalid config value: {key} = {value}")]
    InvalidConfig {
        code: ErrorCode,
        key: String,
        value: String,
    },

    // =========================================================================
    // Server State Errors (SV-STATE)
    // =========================================================================
    /// 서버 미실행
    #[error("[{code}] Server not running")]
    ServerNotRunning { code: ErrorCode },

    /// 서버가 이미 실행 중
    #[error("[{code}] Server already running: {message}")]
    ServerAlreadyRunning { code: ErrorCode, message: String },

    /// 포트 충돌
    #[error("[{code}] Port {port} is already in use{}", .process.as_ref().map(|p| format!(" by {}", p)).unwrap_or_default())]
    PortConflict {
        code: ErrorCode,
        port: u16,
        process: Option<String>,
    },

    // =========================================================================
    // Network Errors
    // =========================================================================
    /// 네트워크 연결 에러
    #[error("[{code}] Network error: {message}")]
    Network { code: ErrorCode, message: String },

    /// DNS 해석 실패
    #[error("[{code}] DNS resolution failed: {host}")]
    DnsResolution { code: ErrorCode, host: String },

    /// 연결 타임아웃
    #[error("[{code}] Connection timeout: {message}")]
    ConnectionTimeout { code: ErrorCode, message: String },

    // =========================================================================
    // General Errors
    // =========================================================================
    /// 아직 구현되지 않음
    #[error("[{code}] Not implemented: {feature}")]
    NotImplemented { code: ErrorCode, feature: String },

    /// 리소스를 찾을 수 없음
    #[error("[{code}] Not found: {resource}")]
    NotFound { code: ErrorCode, resource: String },

    /// 이미 존재함
    #[error("[{code}] Already exists: {resource}")]
    AlreadyExists { code: ErrorCode, resource: String },

    /// 잘못된 인자
    #[error("[{code}] Invalid argument: {message}")]
    InvalidArgument { code: ErrorCode, message: String },

    /// 내부 오류
    #[error("[{code}] Internal error: {message}")]
    Internal { code: ErrorCode, message: String },

    /// 외부 라이브러리 에러 래핑
    #[error("{message}: {source}")]
    External {
        code: ErrorCode,
        message: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl ZetaError {
    // =========================================================================
    // Code Accessor
    // =========================================================================

    /// 에러 코드 반환
    pub fn code(&self) -> &ErrorCode {
        match self {
            Self::Node { code, .. } => code,
            Self::NodeNotFound { code, .. } => code,
            Self::Edge { code, .. } => code,
            Self::EdgeNotFound { code, .. } => code,
            Self::Property { code, .. } => code,
            Self::Label { code, .. } => code,
            Self::Index { code, .. } => code,
            Self::Constraint { code, .. } => code,
            Self::StorageIo { code, .. } => code,
            Self::StorageCorruption { code, .. } => code,
            Self::StorageCapacity { code, .. } => code,
            Self::Serialization { code, .. } => code,
            Self::MemoryAdmission { code, .. } => code,
            Self::Transaction { code, .. } => code,
            Self::TransactionNotFound { code, .. } => code,
            Self::LockTimeout { code, .. } => code,
            Self::Deadlock { code, .. } => code,
            Self::WriteConflict { code, .. } => code,
            Self::WalCorrupted { code, .. } => code,
            Self::LockFailed { code, .. } => code,
            Self::QueryParse { code, .. } => code,
            Self::QueryExecution { code, .. } => code,
            Self::QueryTimeout { code, .. } => code,
            Self::TypeMismatch { code, .. } => code,
            Self::Procedure { code, .. } => code,
            Self::Fulltext { code, .. } => code,
            Self::VectorIndex { code, .. } => code,
            Self::DimensionMismatch { code, .. } => code,
            Self::CacheSerialization { code, .. } => code,
            Self::CacheStorage { code, .. } => code,
            Self::CacheInit { code, .. } => code,
            Self::Http { code, .. } => code,
            Self::Bolt { code, .. } => code,
            Self::Connection { code, .. } => code,
            Self::AuthFailed { code, .. } => code,
            Self::PermissionDenied { code, .. } => code,
            Self::TokenExpired { code } => code,
            Self::ConfigParse { code, .. } => code,
            Self::InvalidConfig { code, .. } => code,
            Self::ServerNotRunning { code } => code,
            Self::ServerAlreadyRunning { code, .. } => code,
            Self::PortConflict { code, .. } => code,
            Self::Network { code, .. } => code,
            Self::DnsResolution { code, .. } => code,
            Self::ConnectionTimeout { code, .. } => code,
            Self::NotImplemented { code, .. } => code,
            Self::NotFound { code, .. } => code,
            Self::AlreadyExists { code, .. } => code,
            Self::InvalidArgument { code, .. } => code,
            Self::Internal { code, .. } => code,
            Self::External { code, .. } => code,
        }
    }

    // =========================================================================
    // Builder Methods - Graph
    // =========================================================================

    /// 노드 에러 생성
    pub fn node(message: impl Into<String>) -> Self {
        Self::Node {
            code: codes::graph::NODE_NOT_FOUND,
            message: message.into(),
        }
    }

    /// 노드를 찾을 수 없음
    pub fn node_not_found(id: u64) -> Self {
        Self::NodeNotFound {
            code: codes::graph::NODE_NOT_FOUND,
            id,
        }
    }

    /// 엣지를 찾을 수 없음
    pub fn edge_not_found(id: u64) -> Self {
        Self::EdgeNotFound {
            code: codes::graph::EDGE_NOT_FOUND,
            id,
        }
    }

    /// 스토리지 I/O 에러 생성
    pub fn storage_io(message: impl Into<String>) -> Self {
        Self::StorageIo {
            code: codes::graph::STORAGE_IO,
            message: message.into(),
        }
    }

    /// 스토리지 I/O 에러 생성 (코드 지정)
    pub fn storage_io_with_code(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::StorageIo {
            code,
            message: message.into(),
        }
    }

    /// 스토리지 용량 초과 에러 생성
    pub fn storage_capacity(message: impl Into<String>) -> Self {
        Self::StorageCapacity {
            code: codes::graph::BUFFER_POOL_EXHAUSTED,
            message: message.into(),
        }
    }

    /// Retryable database-scoped memory admission rejection.
    pub fn memory_admission(message: impl Into<String>) -> Self {
        Self::MemoryAdmission {
            code: codes::graph::MEMORY_ADMISSION_DENIED,
            message: message.into(),
        }
    }

    /// 직렬화 에러 생성
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::Serialization {
            code: codes::graph::SERIALIZATION_FAILED,
            message: message.into(),
        }
    }

    /// 직렬화 에러 생성 (코드 지정)
    pub fn serialization_with_code(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Serialization {
            code,
            message: message.into(),
        }
    }

    /// 인덱스 에러 생성
    pub fn index(message: impl Into<String>) -> Self {
        Self::Index {
            code: codes::graph::INDEX_NOT_FOUND,
            message: message.into(),
        }
    }

    /// 제약조건 위반 에러 생성
    pub fn constraint(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Constraint {
            code: codes::graph::CONSTRAINT_VIOLATION,
            constraint_name: name.into(),
            message: message.into(),
        }
    }

    // =========================================================================
    // Builder Methods - Transaction
    // =========================================================================

    /// 트랜잭션 에러 생성
    pub fn transaction(message: impl Into<String>) -> Self {
        Self::Transaction {
            code: codes::transaction::TX_NOT_FOUND,
            message: message.into(),
        }
    }

    /// 트랜잭션 에러 생성 (코드 지정)
    pub fn transaction_with_code(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Transaction {
            code,
            message: message.into(),
        }
    }

    /// 트랜잭션을 찾을 수 없음
    pub fn transaction_not_found(tx_id: u64) -> Self {
        Self::TransactionNotFound {
            code: codes::transaction::TX_NOT_FOUND,
            tx_id,
        }
    }

    /// 락 획득 실패
    pub fn lock_failed(message: impl Into<String>) -> Self {
        Self::LockFailed {
            code: codes::transaction::LOCK_FAILED,
            message: message.into(),
        }
    }

    /// 락 획득 실패 (코드 지정)
    pub fn lock_failed_with_code(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::LockFailed {
            code,
            message: message.into(),
        }
    }

    /// 락 타임아웃 에러 생성
    pub fn lock_timeout(resource: impl Into<String>, waited_ms: u64) -> Self {
        Self::LockTimeout {
            code: codes::transaction::LOCK_TIMEOUT,
            resource: resource.into(),
            waited_ms,
        }
    }

    /// 데드락 에러 생성
    pub fn deadlock(cycle: Vec<u64>) -> Self {
        Self::Deadlock {
            code: codes::transaction::DEADLOCK,
            cycle,
        }
    }

    /// 쓰기 충돌 에러 생성
    pub fn write_conflict(key: impl Into<String>, conflicting_tx: u64) -> Self {
        Self::WriteConflict {
            code: codes::transaction::WRITE_CONFLICT,
            key: key.into(),
            conflicting_tx,
        }
    }

    /// WAL 손상 에러 생성
    pub fn wal_corrupted(position: u64, details: impl Into<String>) -> Self {
        Self::WalCorrupted {
            code: codes::transaction::WAL_CORRUPTED,
            position,
            details: details.into(),
        }
    }

    // =========================================================================
    // Builder Methods - Query
    // =========================================================================

    /// 쿼리 파싱 에러 생성
    pub fn query_parse(message: impl Into<String>) -> Self {
        Self::QueryParse {
            code: codes::cypher::SYNTAX_ERROR,
            message: message.into(),
        }
    }

    /// 쿼리 실행 에러 생성
    pub fn query_execution(message: impl Into<String>) -> Self {
        Self::QueryExecution {
            code: codes::cypher::EXECUTION_FAILED,
            message: message.into(),
        }
    }

    /// 쿼리 실행 에러 생성 (코드 지정)
    pub fn query_execution_with_code(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::QueryExecution {
            code,
            message: message.into(),
        }
    }

    /// 쿼리 타임아웃 에러 생성
    pub fn query_timeout(message: impl Into<String>) -> Self {
        Self::QueryTimeout {
            code: codes::cypher::TIMEOUT,
            message: message.into(),
        }
    }

    /// 타입 불일치 에러 생성
    pub fn type_mismatch(expected: impl Into<String>, found: impl Into<String>) -> Self {
        Self::TypeMismatch {
            code: codes::cypher::TYPE_MISMATCH,
            expected: expected.into(),
            found: found.into(),
        }
    }

    // =========================================================================
    // Builder Methods - Vector
    // =========================================================================

    /// 벡터 인덱스 에러 생성
    pub fn vector_index(message: impl Into<String>) -> Self {
        Self::VectorIndex {
            code: codes::vector::INDEX_NOT_FOUND,
            message: message.into(),
        }
    }

    /// 차원 불일치 에러 생성
    pub fn dimension_mismatch(expected: usize, found: usize) -> Self {
        Self::DimensionMismatch {
            code: codes::vector::DIMENSION_MISMATCH,
            expected,
            found,
        }
    }

    // =========================================================================
    // Builder Methods - Cache
    // =========================================================================

    /// 캐시 직렬화 에러 생성
    pub fn cache_serialization(message: impl Into<String>) -> Self {
        Self::CacheSerialization {
            code: codes::cache::SERIALIZATION_FAILED,
            message: message.into(),
        }
    }

    /// 캐시 저장소 에러 생성
    pub fn cache_storage(message: impl Into<String>) -> Self {
        Self::CacheStorage {
            code: codes::cache::STORAGE_ERROR,
            message: message.into(),
        }
    }

    /// 캐시 초기화 에러 생성
    pub fn cache_init(message: impl Into<String>) -> Self {
        Self::CacheInit {
            code: codes::cache::INIT_FAILED,
            message: message.into(),
        }
    }

    // =========================================================================
    // Builder Methods - Server
    // =========================================================================

    /// 인증 실패 에러 생성
    pub fn auth_failed(message: impl Into<String>) -> Self {
        Self::AuthFailed {
            code: codes::server::AUTH_FAILED,
            message: message.into(),
        }
    }

    /// 권한 없음 에러 생성
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::PermissionDenied {
            code: codes::server::PERMISSION_DENIED,
            message: message.into(),
        }
    }

    /// 토큰 만료 에러 생성
    pub fn token_expired() -> Self {
        Self::TokenExpired {
            code: codes::server::TOKEN_EXPIRED,
        }
    }

    /// HTTP 에러 생성
    pub fn http(status: u16, message: impl Into<String>) -> Self {
        let code = match status {
            400 => codes::server::HTTP_BAD_REQUEST,
            401 => codes::server::HTTP_UNAUTHORIZED,
            403 => codes::server::HTTP_FORBIDDEN,
            404 => codes::server::HTTP_NOT_FOUND,
            _ => codes::server::HTTP_INTERNAL,
        };
        Self::Http {
            code,
            status,
            message: message.into(),
        }
    }

    /// 연결 에러 생성
    pub fn connection(message: impl Into<String>) -> Self {
        Self::Connection {
            code: codes::server::BOLT_CONNECTION_ERROR,
            message: message.into(),
        }
    }

    /// 연결 타임아웃 에러 생성
    pub fn connection_timeout(message: impl Into<String>) -> Self {
        Self::ConnectionTimeout {
            code: codes::server::BOLT_CONNECTION_ERROR,
            message: message.into(),
        }
    }

    /// 설정 파싱 에러 생성
    pub fn config_parse(message: impl Into<String>) -> Self {
        Self::ConfigParse {
            code: codes::server::CONFIG_PARSE_ERROR,
            message: message.into(),
        }
    }

    /// 서버 미실행 에러 생성
    pub fn server_not_running() -> Self {
        Self::ServerNotRunning {
            code: codes::server::SERVER_NOT_RUNNING,
        }
    }

    /// 서버 이미 실행 중 에러 생성
    pub fn server_already_running(message: impl Into<String>) -> Self {
        Self::ServerAlreadyRunning {
            code: codes::server::SERVER_ALREADY_RUNNING,
            message: message.into(),
        }
    }

    /// 포트 충돌 에러 생성
    pub fn port_conflict(port: u16, process: Option<String>) -> Self {
        Self::PortConflict {
            code: codes::server::PORT_CONFLICT,
            port,
            process,
        }
    }

    // =========================================================================
    // Builder Methods - General
    // =========================================================================

    /// 리소스 없음 에러 생성
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound {
            code: codes::graph::NODE_NOT_FOUND,
            resource: resource.into(),
        }
    }

    /// 이미 존재 에러 생성
    pub fn already_exists(resource: impl Into<String>) -> Self {
        Self::AlreadyExists {
            code: codes::graph::CONSTRAINT_VIOLATION,
            resource: resource.into(),
        }
    }

    /// 미구현 에러 생성
    pub fn not_implemented(feature: impl Into<String>) -> Self {
        Self::NotImplemented {
            code: codes::graph::NOT_IMPLEMENTED,
            feature: feature.into(),
        }
    }

    /// 내부 오류 생성
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            code: codes::graph::SYSTEM_ERROR,
            message: message.into(),
        }
    }

    /// 잘못된 인자 에러 생성
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument {
            code: codes::cypher::INVALID_QUERY,
            message: message.into(),
        }
    }

    // =========================================================================
    // Conversion Methods
    // =========================================================================

    /// 에러 코드 반환 (HTTP 상태 코드 스타일)
    pub fn http_status(&self) -> u16 {
        if let Self::QueryExecution { code, .. } = self {
            if code == &codes::cypher::ADMIN_SERVICE_UNAVAILABLE {
                return 503;
            }
            if code == &codes::cypher::ADMIN_UNSUPPORTED {
                return 501;
            }
            if code == &codes::cypher::ADMIN_PRINCIPAL_REQUIRED {
                return 401;
            }
            if code == &codes::cypher::ADMIN_STATE_INVARIANT {
                return 500;
            }
        }

        match self {
            // 400 Bad Request
            Self::QueryParse { .. }
            | Self::TypeMismatch { .. }
            | Self::InvalidConfig { .. }
            | Self::InvalidArgument { .. }
            | Self::DimensionMismatch { .. } => 400,

            // 401 Unauthorized
            Self::AuthFailed { .. } | Self::TokenExpired { .. } => 401,

            // 403 Forbidden
            Self::PermissionDenied { .. } => 403,

            // 404 Not Found
            Self::NotFound { .. } | Self::NodeNotFound { .. } | Self::EdgeNotFound { .. } => 404,

            // 408 Timeout
            Self::QueryTimeout { .. }
            | Self::ConnectionTimeout { .. }
            | Self::LockTimeout { .. } => 408,

            // 409 Conflict
            Self::AlreadyExists { .. }
            | Self::Constraint { .. }
            | Self::WriteConflict { .. }
            | Self::Deadlock { .. }
            | Self::ServerAlreadyRunning { .. }
            | Self::PortConflict { .. } => 409,

            // 500 Internal Server Error
            Self::Internal { .. }
            | Self::StorageCorruption { .. }
            | Self::WalCorrupted { .. }
            | Self::External { .. } => 500,

            // 501 Not Implemented
            Self::NotImplemented { .. } => 501,

            // 503 Service Unavailable
            Self::ServerNotRunning { .. } | Self::MemoryAdmission { .. } => 503,

            // Default to 500
            _ => 500,
        }
    }

    /// 재시도 가능 여부
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::QueryTimeout { .. }
                | Self::ConnectionTimeout { .. }
                | Self::LockTimeout { .. }
                | Self::Deadlock { .. }
                | Self::WriteConflict { .. }
                | Self::Network { .. }
                | Self::Connection { .. }
                | Self::MemoryAdmission { .. }
        )
    }

    /// 트랜잭션 롤백 필요 여부
    pub fn requires_rollback(&self) -> bool {
        matches!(
            self,
            Self::Deadlock { .. }
                | Self::WriteConflict { .. }
                | Self::WalCorrupted { .. }
                | Self::Constraint { .. }
        )
    }

    /// 클라이언트 에러 여부 (4xx)
    pub fn is_client_error(&self) -> bool {
        let status = self.http_status();
        (400..500).contains(&status)
    }

    /// 서버 에러 여부 (5xx)
    pub fn is_server_error(&self) -> bool {
        let status = self.http_status();
        status >= 500
    }

    /// CLI 종료 코드 반환
    ///
    /// CLI 도구에서 프로세스 종료 시 사용할 코드를 반환합니다.
    pub fn exit_code(&self) -> std::process::ExitCode {
        use std::process::ExitCode;
        match self {
            // 2: 설정/검증 에러
            Self::ConfigParse { .. }
            | Self::InvalidConfig { .. }
            | Self::InvalidArgument { .. }
            | Self::QueryParse { .. } => ExitCode::from(2),

            // 3: 인증/권한 에러
            Self::AuthFailed { .. } | Self::TokenExpired { .. } | Self::PermissionDenied { .. } => {
                ExitCode::from(3)
            }

            // 4: 서버 상태 에러
            Self::ServerNotRunning { .. } | Self::ServerAlreadyRunning { .. } => ExitCode::from(4),

            // 5: 연결/타임아웃 에러
            Self::Connection { .. }
            | Self::ConnectionTimeout { .. }
            | Self::Network { .. }
            | Self::QueryTimeout { .. } => ExitCode::from(5),

            // 6: 포트 충돌
            Self::PortConflict { .. } => ExitCode::from(6),

            // 7: 쿼리 실행 에러
            Self::QueryExecution { .. } => ExitCode::from(7),

            // 8: 리소스 없음
            Self::NotFound { .. } | Self::NodeNotFound { .. } | Self::EdgeNotFound { .. } => {
                ExitCode::from(8)
            }

            // 1: 기타 모든 에러
            _ => ExitCode::FAILURE,
        }
    }
}

// =========================================================================
// From Implementations
// =========================================================================

impl From<std::io::Error> for ZetaError {
    fn from(err: std::io::Error) -> Self {
        Self::StorageIo {
            code: codes::graph::STORAGE_IO,
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for ZetaError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization {
            code: codes::graph::SERIALIZATION_FAILED,
            message: err.to_string(),
        }
    }
}

// =============================================================================
// gRPC Error Conversion (optional tonic feature)
// =============================================================================

#[cfg(feature = "grpc")]
impl From<ZetaError> for tonic::Status {
    fn from(err: ZetaError) -> Self {
        match &err {
            ZetaError::AuthFailed { .. } | ZetaError::TokenExpired { .. } => {
                tonic::Status::unauthenticated(err.to_string())
            }
            ZetaError::PermissionDenied { .. } => tonic::Status::permission_denied(err.to_string()),
            ZetaError::NotFound { .. }
            | ZetaError::NodeNotFound { .. }
            | ZetaError::EdgeNotFound { .. } => tonic::Status::not_found(err.to_string()),
            ZetaError::QueryParse { .. }
            | ZetaError::InvalidArgument { .. }
            | ZetaError::InvalidConfig { .. }
            | ZetaError::TypeMismatch { .. }
            | ZetaError::DimensionMismatch { .. } => {
                tonic::Status::invalid_argument(err.to_string())
            }
            ZetaError::AlreadyExists { .. }
            | ZetaError::Constraint { .. }
            | ZetaError::WriteConflict { .. }
            | ZetaError::Deadlock { .. } => tonic::Status::already_exists(err.to_string()),
            ZetaError::QueryTimeout { .. }
            | ZetaError::LockTimeout { .. }
            | ZetaError::ConnectionTimeout { .. } => {
                tonic::Status::deadline_exceeded(err.to_string())
            }
            ZetaError::NotImplemented { .. } => tonic::Status::unimplemented(err.to_string()),
            ZetaError::ServerNotRunning { .. } => tonic::Status::unavailable(err.to_string()),
            _ => tonic::Status::internal(err.to_string()),
        }
    }
}

#[cfg(test)]
mod admin_error_tests {
    use super::*;

    #[test]
    fn admin_error_codes_keep_their_http_contract() {
        let cases = [
            (codes::cypher::ADMIN_SERVICE_UNAVAILABLE, 503),
            (codes::cypher::ADMIN_UNSUPPORTED, 501),
            (codes::cypher::ADMIN_PRINCIPAL_REQUIRED, 401),
            (codes::cypher::ADMIN_STATE_INVARIANT, 500),
        ];
        for (code, expected_status) in cases {
            let error = ZetaError::query_execution_with_code(code.clone(), "test");
            assert_eq!(error.code(), &code);
            assert_eq!(error.http_status(), expected_status);
        }
    }
}
