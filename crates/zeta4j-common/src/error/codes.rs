//! 에러 코드 상수 정의
//!
//! 모든 에러는 구조화된 코드를 가지며, 이를 통해 에러를 분류하고 추적할 수 있습니다.
//!
//! # 코드 형식
//!
//! `{DOMAIN}-{CATEGORY}-{CODE}`
//!
//! - DOMAIN: 2자리 도메인 코드 (GR, TX, CY, VE, SV 등)
//! - CATEGORY: 카테고리 이름 (NODE, LOCK, SYNTAX 등)
//! - CODE: 3자리 숫자 코드
//!
//! # 도메인 목록
//!
//! | 도메인 | 설명 |
//! |--------|------|
//! | GR | Graph Engine |
//! | TX | Transaction |
//! | CY | Cypher |
//! | FT | Fulltext |
//! | VE | Vector |
//! | CA | Cache |
//! | SV | Server |

use std::fmt;

/// 구조화된 에러 코드
///
/// 에러를 도메인/카테고리/코드로 분류합니다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ErrorCode {
    /// 도메인 코드 (2자리: GR, TX, CY 등)
    pub domain: &'static str,
    /// 카테고리 (NODE, LOCK, SYNTAX 등)
    pub category: &'static str,
    /// 숫자 코드
    pub code: u16,
}

impl ErrorCode {
    /// 새 에러 코드 생성
    pub const fn new(domain: &'static str, category: &'static str, code: u16) -> Self {
        Self {
            domain,
            category,
            code,
        }
    }

    /// 전체 코드 문자열 반환 (예: "GR-NODE-001")
    pub fn full_code(&self) -> String {
        format!("{}-{}-{:03}", self.domain, self.category, self.code)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{:03}", self.domain, self.category, self.code)
    }
}

// =============================================================================
// Graph Engine Errors (GR)
// =============================================================================

/// 그래프 엔진 에러 코드
pub mod graph {
    use super::ErrorCode;

    // Node 관련
    pub const NODE_NOT_FOUND: ErrorCode = ErrorCode::new("GR", "NODE", 1);
    pub const NODE_CREATE_FAILED: ErrorCode = ErrorCode::new("GR", "NODE", 2);
    pub const NODE_UPDATE_FAILED: ErrorCode = ErrorCode::new("GR", "NODE", 3);
    pub const NODE_DELETE_FAILED: ErrorCode = ErrorCode::new("GR", "NODE", 4);

    // Edge 관련
    pub const EDGE_NOT_FOUND: ErrorCode = ErrorCode::new("GR", "EDGE", 1);
    pub const EDGE_CREATE_FAILED: ErrorCode = ErrorCode::new("GR", "EDGE", 2);
    pub const EDGE_UPDATE_FAILED: ErrorCode = ErrorCode::new("GR", "EDGE", 3);
    pub const EDGE_DELETE_FAILED: ErrorCode = ErrorCode::new("GR", "EDGE", 4);

    // Property 관련
    pub const PROPERTY_NOT_FOUND: ErrorCode = ErrorCode::new("GR", "PROP", 1);
    pub const PROPERTY_TYPE_MISMATCH: ErrorCode = ErrorCode::new("GR", "PROP", 2);
    pub const PROPERTY_INVALID: ErrorCode = ErrorCode::new("GR", "PROP", 3);

    // Label 관련
    pub const LABEL_NOT_FOUND: ErrorCode = ErrorCode::new("GR", "LABEL", 1);
    pub const LABEL_INVALID: ErrorCode = ErrorCode::new("GR", "LABEL", 2);

    // Index 관련
    pub const INDEX_NOT_FOUND: ErrorCode = ErrorCode::new("GR", "INDEX", 1);
    pub const INDEX_CREATE_FAILED: ErrorCode = ErrorCode::new("GR", "INDEX", 2);
    pub const INDEX_CORRUPTED: ErrorCode = ErrorCode::new("GR", "INDEX", 3);

    // Constraint 관련
    pub const CONSTRAINT_VIOLATION: ErrorCode = ErrorCode::new("GR", "CONST", 1);
    pub const CONSTRAINT_NOT_FOUND: ErrorCode = ErrorCode::new("GR", "CONST", 2);

    // Storage 관련
    pub const STORAGE_IO: ErrorCode = ErrorCode::new("GR", "STOR", 1);
    pub const STORAGE_CORRUPTION: ErrorCode = ErrorCode::new("GR", "STOR", 2);
    pub const STORAGE_CAPACITY: ErrorCode = ErrorCode::new("GR", "STOR", 3);

    // Database-scoped memory admission
    pub const MEMORY_ADMISSION_DENIED: ErrorCode = ErrorCode::new("GR", "MEM", 1);

    // Serialization 관련
    pub const SERIALIZATION_FAILED: ErrorCode = ErrorCode::new("GR", "SER", 1);
    pub const DESERIALIZATION_FAILED: ErrorCode = ErrorCode::new("GR", "SER", 2);

    // System 관련
    pub const BUFFER_POOL_EXHAUSTED: ErrorCode = ErrorCode::new("GR", "SYS", 1);
    pub const NOT_IMPLEMENTED: ErrorCode = ErrorCode::new("GR", "SYS", 2);
    pub const SYSTEM_ERROR: ErrorCode = ErrorCode::new("GR", "SYS", 99);
}

// =============================================================================
// Transaction Errors (TX)
// =============================================================================

/// 트랜잭션 에러 코드
pub mod transaction {
    use super::ErrorCode;

    // 상태 관련
    pub const TX_NOT_FOUND: ErrorCode = ErrorCode::new("TX", "STATE", 1);
    pub const TX_ALREADY_COMMITTED: ErrorCode = ErrorCode::new("TX", "STATE", 2);
    pub const TX_ALREADY_ABORTED: ErrorCode = ErrorCode::new("TX", "STATE", 3);
    pub const TX_INVALID_STATE: ErrorCode = ErrorCode::new("TX", "STATE", 4);

    // Lock 관련
    pub const LOCK_FAILED: ErrorCode = ErrorCode::new("TX", "LOCK", 1);
    pub const LOCK_TIMEOUT: ErrorCode = ErrorCode::new("TX", "LOCK", 2);
    pub const LOCK_UPGRADE_FAILED: ErrorCode = ErrorCode::new("TX", "LOCK", 3);
    pub const LOCK_POISONED: ErrorCode = ErrorCode::new("TX", "LOCK", 4);
    pub const DEADLOCK: ErrorCode = ErrorCode::new("TX", "LOCK", 5);

    // WAL 관련
    pub const WAL_CORRUPTED: ErrorCode = ErrorCode::new("TX", "WAL", 1);
    pub const WAL_WRITE_FAILED: ErrorCode = ErrorCode::new("TX", "WAL", 2);
    pub const WAL_SYNC_FAILED: ErrorCode = ErrorCode::new("TX", "WAL", 3);

    // MVCC 관련
    pub const WRITE_CONFLICT: ErrorCode = ErrorCode::new("TX", "MVCC", 1);
    pub const VERSION_NOT_FOUND: ErrorCode = ErrorCode::new("TX", "MVCC", 2);
    pub const MVCC_CONFLICT: ErrorCode = ErrorCode::new("TX", "MVCC", 3);
}

// =============================================================================
// Cypher Errors (CY)
// =============================================================================

/// Cypher 에러 코드
pub mod cypher {
    use super::ErrorCode;

    // Parse 관련
    pub const SYNTAX_ERROR: ErrorCode = ErrorCode::new("CY", "PARSE", 1);
    pub const SEMANTIC_ERROR: ErrorCode = ErrorCode::new("CY", "PARSE", 2);
    pub const INVALID_QUERY: ErrorCode = ErrorCode::new("CY", "PARSE", 3);

    // Execution 관련
    pub const EXECUTION_FAILED: ErrorCode = ErrorCode::new("CY", "EXEC", 1);
    pub const TIMEOUT: ErrorCode = ErrorCode::new("CY", "EXEC", 2);
    pub const RESOURCE_EXHAUSTED: ErrorCode = ErrorCode::new("CY", "EXEC", 3);
    pub const PARAMETER_NOT_FOUND: ErrorCode = ErrorCode::new("CY", "EXEC", 10);
    pub const VARIABLE_NOT_FOUND: ErrorCode = ErrorCode::new("CY", "EXEC", 11);
    pub const PROPERTY_NOT_FOUND: ErrorCode = ErrorCode::new("CY", "EXEC", 12);
    pub const INDEX_ACCESS_FAILED: ErrorCode = ErrorCode::new("CY", "EXEC", 13);
    pub const FUNCTION_EVAL_FAILED: ErrorCode = ErrorCode::new("CY", "EXEC", 14);
    pub const INVALID_ARGUMENT_COUNT: ErrorCode = ErrorCode::new("CY", "EXEC", 15);
    pub const NULL_VALUE: ErrorCode = ErrorCode::new("CY", "EXEC", 16);

    // Procedure 관련
    pub const PROCEDURE_NOT_FOUND: ErrorCode = ErrorCode::new("CY", "PROC", 1);
    pub const PROCEDURE_INVALID_ARGS: ErrorCode = ErrorCode::new("CY", "PROC", 2);
    pub const PROCEDURE_EXECUTION_FAILED: ErrorCode = ErrorCode::new("CY", "PROC", 3);

    // Type 관련
    pub const TYPE_MISMATCH: ErrorCode = ErrorCode::new("CY", "TYPE", 1);
    pub const INVALID_TYPE: ErrorCode = ErrorCode::new("CY", "TYPE", 2);

    // Cursor 관련
    pub const CURSOR_NOT_FOUND: ErrorCode = ErrorCode::new("CY", "CURSOR", 1);
    pub const CURSOR_EXPIRED: ErrorCode = ErrorCode::new("CY", "CURSOR", 2);
    pub const CURSOR_CLOSED: ErrorCode = ErrorCode::new("CY", "CURSOR", 3);
    pub const CURSOR_EXHAUSTED: ErrorCode = ErrorCode::new("CY", "CURSOR", 4);

    // Config 관련 (페이지네이션 설정 등)
    pub const INVALID_PAGE_SIZE: ErrorCode = ErrorCode::new("CY", "CONFIG", 1);
    pub const INVALID_CURSOR_LIMIT: ErrorCode = ErrorCode::new("CY", "CONFIG", 2);
    pub const INVALID_TTL: ErrorCode = ErrorCode::new("CY", "CONFIG", 3);

    // Admin execution
    pub const ADMIN_SERVICE_UNAVAILABLE: ErrorCode = ErrorCode::new("CY", "ADMIN", 1);
    pub const ADMIN_UNSUPPORTED: ErrorCode = ErrorCode::new("CY", "ADMIN", 2);
    pub const ADMIN_PRINCIPAL_REQUIRED: ErrorCode = ErrorCode::new("CY", "ADMIN", 3);
    pub const ADMIN_STATE_INVARIANT: ErrorCode = ErrorCode::new("CY", "ADMIN", 4);
}

// =============================================================================
// Fulltext Errors (FT)
// =============================================================================

/// 전문 검색 에러 코드
pub mod fulltext {
    use super::ErrorCode;

    pub const INDEX_NOT_FOUND: ErrorCode = ErrorCode::new("FT", "INDEX", 1);
    pub const INDEX_CORRUPTED: ErrorCode = ErrorCode::new("FT", "INDEX", 2);
    pub const FIELD_NOT_FOUND: ErrorCode = ErrorCode::new("FT", "FIELD", 1);
    pub const QUERY_PARSE_FAILED: ErrorCode = ErrorCode::new("FT", "QUERY", 1);
    pub const SCHEMA_ERROR: ErrorCode = ErrorCode::new("FT", "SCHEMA", 1);
    pub const TOKENIZER_ERROR: ErrorCode = ErrorCode::new("FT", "TOKEN", 1);
    pub const TANTIVY_ERROR: ErrorCode = ErrorCode::new("FT", "SYS", 1);
    pub const INVALID_CONFIG: ErrorCode = ErrorCode::new("FT", "CONFIG", 1);
    pub const RECOVERY_FAILED: ErrorCode = ErrorCode::new("FT", "RECOV", 1);
}

// =============================================================================
// Vector Errors (VE)
// =============================================================================

/// 벡터 인덱스 에러 코드
pub mod vector {
    use super::ErrorCode;

    // Index 관련
    pub const INDEX_NOT_FOUND: ErrorCode = ErrorCode::new("VE", "INDEX", 1);
    pub const INDEX_CREATE_FAILED: ErrorCode = ErrorCode::new("VE", "INDEX", 2);
    pub const INDEX_CORRUPTED: ErrorCode = ErrorCode::new("VE", "INDEX", 3);

    // Dimension 관련
    pub const DIMENSION_MISMATCH: ErrorCode = ErrorCode::new("VE", "DIM", 1);
    pub const INVALID_DIMENSION: ErrorCode = ErrorCode::new("VE", "DIM", 2);

    // HNSW 관련
    pub const HNSW_BUILD_FAILED: ErrorCode = ErrorCode::new("VE", "HNSW", 1);
    pub const HNSW_SEARCH_FAILED: ErrorCode = ErrorCode::new("VE", "HNSW", 2);
    pub const HNSW_PERSISTENCE_FAILED: ErrorCode = ErrorCode::new("VE", "HNSW", 3);
    pub const HNSW_RECOVERY_FAILED: ErrorCode = ErrorCode::new("VE", "HNSW", 4);

    // Storage 관련
    pub const STORAGE_ERROR: ErrorCode = ErrorCode::new("VE", "STOR", 1);
    pub const VECTOR_NOT_FOUND: ErrorCode = ErrorCode::new("VE", "STOR", 2);
    pub const DISK_STORAGE_ERROR: ErrorCode = ErrorCode::new("VE", "STOR", 3);

    // Benchmark 관련
    pub const BENCHMARK_DATASET_ERROR: ErrorCode = ErrorCode::new("VE", "BENCH", 1);
    pub const BENCHMARK_CONFIG_ERROR: ErrorCode = ErrorCode::new("VE", "BENCH", 2);
    pub const BENCHMARK_INDEX_ERROR: ErrorCode = ErrorCode::new("VE", "BENCH", 3);
}

// =============================================================================
// Cache Errors (CA)
// =============================================================================

/// 캐시 에러 코드
pub mod cache {
    use super::ErrorCode;

    // Serialization 관련
    pub const SERIALIZATION_FAILED: ErrorCode = ErrorCode::new("CA", "SER", 1);
    pub const DESERIALIZATION_FAILED: ErrorCode = ErrorCode::new("CA", "SER", 2);

    // Storage 관련
    pub const STORAGE_ERROR: ErrorCode = ErrorCode::new("CA", "STOR", 1);
    pub const STORAGE_OPEN_FAILED: ErrorCode = ErrorCode::new("CA", "STOR", 2);

    // Init 관련
    pub const INIT_FAILED: ErrorCode = ErrorCode::new("CA", "INIT", 1);
}

// =============================================================================
// Server Errors (SV)
// =============================================================================

/// 서버 에러 코드
pub mod server {
    use super::ErrorCode;

    // HTTP 관련
    pub const HTTP_BAD_REQUEST: ErrorCode = ErrorCode::new("SV", "HTTP", 400);
    pub const HTTP_UNAUTHORIZED: ErrorCode = ErrorCode::new("SV", "HTTP", 401);
    pub const HTTP_FORBIDDEN: ErrorCode = ErrorCode::new("SV", "HTTP", 403);
    pub const HTTP_NOT_FOUND: ErrorCode = ErrorCode::new("SV", "HTTP", 404);
    pub const HTTP_INTERNAL: ErrorCode = ErrorCode::new("SV", "HTTP", 500);

    // Bolt 관련
    pub const BOLT_PROTOCOL_ERROR: ErrorCode = ErrorCode::new("SV", "BOLT", 1);
    pub const BOLT_CONNECTION_ERROR: ErrorCode = ErrorCode::new("SV", "BOLT", 2);

    // Auth 관련
    pub const AUTH_FAILED: ErrorCode = ErrorCode::new("SV", "AUTH", 1);
    pub const TOKEN_EXPIRED: ErrorCode = ErrorCode::new("SV", "AUTH", 2);
    pub const PERMISSION_DENIED: ErrorCode = ErrorCode::new("SV", "AUTH", 3);
    pub const PRINCIPAL_INACTIVE: ErrorCode = ErrorCode::new("SV", "AUTH", 4);
    pub const DATABASE_NOT_FOUND: ErrorCode = ErrorCode::new("SV", "AUTH", 5);
    pub const ACCESS_INVARIANT: ErrorCode = ErrorCode::new("SV", "AUTH", 6);

    // Config 관련
    pub const CONFIG_PARSE_ERROR: ErrorCode = ErrorCode::new("SV", "CONFIG", 1);
    pub const CONFIG_INVALID: ErrorCode = ErrorCode::new("SV", "CONFIG", 2);

    // Secure bootstrap/runtime profile
    pub const BOOTSTRAP_INSECURE_PROFILE: ErrorCode = ErrorCode::new("SV", "BOOT", 1);
    pub const BOOTSTRAP_NON_LOOPBACK: ErrorCode = ErrorCode::new("SV", "BOOT", 2);
    pub const BOOTSTRAP_MISSING_SECRET: ErrorCode = ErrorCode::new("SV", "BOOT", 3);
    pub const BOOTSTRAP_INVALID_SECRET: ErrorCode = ErrorCode::new("SV", "BOOT", 4);
    pub const BOOTSTRAP_CREDENTIAL_REQUIRED: ErrorCode = ErrorCode::new("SV", "BOOT", 6);
    pub const BOOTSTRAP_CREDENTIAL_EXPIRED: ErrorCode = ErrorCode::new("SV", "BOOT", 7);
    pub const BOOTSTRAP_CREDENTIAL_CONSUMED: ErrorCode = ErrorCode::new("SV", "BOOT", 8);
    pub const BOOTSTRAP_UNSAFE_PUBLIC_PATH: ErrorCode = ErrorCode::new("SV", "BOOT", 9);
    pub const BOOTSTRAP_INVALID_PROFILE: ErrorCode = ErrorCode::new("SV", "BOOT", 10);

    // TLS transport
    pub const TLS_CONFIGURATION_INVALID: ErrorCode = ErrorCode::new("SV", "TLS", 1);
    pub const TLS_MATERIAL_INVALID: ErrorCode = ErrorCode::new("SV", "TLS", 2);
    pub const TLS_PEER_UNTRUSTED: ErrorCode = ErrorCode::new("SV", "TLS", 3);
    pub const TLS_IDENTITY_MISMATCH: ErrorCode = ErrorCode::new("SV", "TLS", 4);
    pub const TLS_IDENTITY_REVOKED: ErrorCode = ErrorCode::new("SV", "TLS", 6);
    pub const TLS_GENERATION_REJECTED: ErrorCode = ErrorCode::new("SV", "TLS", 9);

    // CLI/서버 상태 관련
    pub const SERVER_NOT_RUNNING: ErrorCode = ErrorCode::new("SV", "STATE", 1);
    pub const SERVER_ALREADY_RUNNING: ErrorCode = ErrorCode::new("SV", "STATE", 2);
    pub const PORT_CONFLICT: ErrorCode = ErrorCode::new("SV", "STATE", 3);
    pub const SERVER_STARTUP_FAILED: ErrorCode = ErrorCode::new("SV", "STATE", 4);
    pub const SERVER_SHUTDOWN_FAILED: ErrorCode = ErrorCode::new("SV", "STATE", 5);
}
