//! 제약조건 관리 명령 AST
//!
//! CREATE/DROP CONSTRAINT 등

use zeta4j_graph::core::Value;

// Re-export from zeta4j_common
pub use zeta4j_common::{CascadeAction, ReferentialAction};

/// 제약조건 관리 명령
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintCommand {
    // ============================================================
    // 기존 제약 (유지)
    // ============================================================
    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop IS UNIQUE
    CreateUnique {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티 목록
        properties: Vec<String>,
    },

    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop IS NOT NULL
    CreateNotNull {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티 목록
        properties: Vec<String>,
    },

    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE (n.a, n.b) IS NODE KEY
    CreateNodeKey {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티 목록
        properties: Vec<String>,
    },

    /// DROP CONSTRAINT name
    Drop {
        /// 제약조건 이름
        name: String,
    },

    /// SHOW CONSTRAINTS
    Show,

    /// SHOW CONSTRAINTS FOR :Label
    ShowFor {
        /// 라벨
        label: String,
    },

    // ============================================================
    // 신규: 프로퍼티 레벨 제약
    // ============================================================
    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop RANGE(min, max)
    CreateRange {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티
        property: String,
        /// 최소값 (None = 제한 없음)
        min: Option<f64>,
        /// 최대값 (None = 제한 없음)
        max: Option<f64>,
        /// 경계 포함 여부
        inclusive: bool,
    },

    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop LENGTH(min, max)
    CreateLength {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티
        property: String,
        /// 최소 길이 (None = 제한 없음)
        min_len: Option<usize>,
        /// 최대 길이 (None = 제한 없음)
        max_len: Option<usize>,
    },

    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop PATTERN('regex')
    CreatePattern {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티
        property: String,
        /// 정규식 패턴
        regex: String,
        /// 에러 메시지 (선택적)
        message: Option<String>,
    },

    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop IN ('a', 'b', 'c')
    CreateEnum {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티
        property: String,
        /// 허용 값 목록
        values: Vec<String>,
    },

    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop IS IMMUTABLE
    CreateImmutable {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티
        property: String,
    },

    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE n.prop DEFAULT value
    CreateDefault {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티
        property: String,
        /// 기본값
        value: Value,
    },

    // ============================================================
    // 신규: 클래스 레벨 제약
    // ============================================================
    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE CHECK(expression)
    CreateCheck {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 검사 표현식
        expression: String,
        /// 에러 메시지 (선택적)
        message: Option<String>,
    },

    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE AT LEAST ONE OF (n.a, n.b)
    CreateRequireAtLeastOne {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티 목록
        properties: Vec<String>,
    },

    /// CREATE CONSTRAINT name FOR (n:Label) REQUIRE MUTUALLY EXCLUSIVE (n.a, n.b)
    CreateMutuallyExclusive {
        /// 제약조건 이름
        name: String,
        /// 라벨
        label: String,
        /// 프로퍼티 목록
        properties: Vec<String>,
    },

    // ============================================================
    // 신규: 관계 레벨 제약
    // ============================================================
    /// CREATE CONSTRAINT name FOR ()-[r:REL]->() REQUIRE CARDINALITY(min, max)
    CreateCardinality {
        /// 제약조건 이름
        name: String,
        /// 관계 타입
        rel_type: String,
        /// 최소 개수
        min: u32,
        /// 최대 개수 (None = 무제한)
        max: Option<u32>,
    },

    /// CREATE CONSTRAINT name FOR ()-[r:REL]->() REQUIRE ON DELETE CASCADE
    CreateCascade {
        /// 제약조건 이름
        name: String,
        /// 관계 타입
        rel_type: String,
        /// 연쇄 동작
        action: CascadeAction,
    },

    /// CREATE CONSTRAINT name FOR ()-[r:REL]->() REQUIRE ON DELETE/UPDATE action
    CreateReferentialIntegrity {
        /// 제약조건 이름
        name: String,
        /// 관계 타입
        rel_type: String,
        /// 삭제 시 동작
        on_delete: ReferentialAction,
        /// 수정 시 동작
        on_update: ReferentialAction,
    },
}
