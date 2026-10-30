//! 관리 명령 AST
//!
//! 사용자, 데이터베이스 관리

/// 관리 명령 - 사용자, 데이터베이스 관리
///
/// Neo4j 호환 관리 명령어를 지원합니다.
///
/// # 지원 명령
///
/// ## 사용자 관리
/// - `CREATE USER name SET PASSWORD 'password'`
/// - `DROP USER name`
/// - `ALTER USER name SET PASSWORD 'new_password'`
/// - `SHOW USERS`
/// - `SHOW CURRENT USER`
///
/// ## 데이터베이스 관리
/// - `CREATE DATABASE name`
/// - `DROP DATABASE name`
/// - `SHOW DATABASES`
/// - `SHOW DEFAULT DATABASE`
///
/// ## 프로시저 관리
/// - `SHOW PROCEDURES`
#[derive(Debug, Clone, PartialEq)]
pub enum AdminCommand {
    // ===== 사용자 관리 =====
    /// CREATE USER name SET PASSWORD 'password' [CHANGE [NOT] REQUIRED]
    CreateUser {
        /// 사용자 이름
        name: String,
        /// 비밀번호 (평문, 저장 시 해시됨)
        password: String,
        /// 비밀번호 변경 필요 여부
        change_required: bool,
        /// IF NOT EXISTS
        if_not_exists: bool,
    },

    /// DROP USER name
    DropUser {
        /// 사용자 이름
        name: String,
        /// IF EXISTS
        if_exists: bool,
    },

    /// ALTER USER name SET PASSWORD 'new_password' [CHANGE [NOT] REQUIRED]
    AlterUser {
        /// 사용자 이름
        name: String,
        /// 새 비밀번호 (선택적)
        password: Option<String>,
        /// 비밀번호 변경 필요 여부 (선택적)
        change_required: Option<bool>,
        /// 사용자 활성화/비활성화 (선택적)
        suspended: Option<bool>,
    },

    /// SHOW USERS
    ShowUsers,

    /// SHOW CURRENT USER
    ShowCurrentUser,

    // ===== 데이터베이스 관리 =====
    /// CREATE DATABASE name
    CreateDatabase {
        /// 데이터베이스 이름
        name: String,
        /// IF NOT EXISTS
        if_not_exists: bool,
    },

    /// DROP DATABASE name
    DropDatabase {
        /// 데이터베이스 이름
        name: String,
        /// IF EXISTS
        if_exists: bool,
    },

    /// SHOW DATABASES
    ShowDatabases,

    /// SHOW DEFAULT DATABASE
    ShowDefaultDatabase,

    /// START DATABASE name
    StartDatabase {
        /// 데이터베이스 이름
        name: String,
    },

    /// STOP DATABASE name
    StopDatabase {
        /// 데이터베이스 이름
        name: String,
    },

    // ===== 프로시저 관리 =====
    /// SHOW PROCEDURES
    ShowProcedures,
}
