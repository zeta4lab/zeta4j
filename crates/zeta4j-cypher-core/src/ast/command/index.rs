// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 인덱스 관리 명령 AST
//!
//! CREATE/DROP INDEX 등

/// 인덱스 관리 명령
#[derive(Debug, Clone, PartialEq)]
pub enum IndexCommand {
    /// CREATE INDEX [name] ON :Label(property, ...)
    Create {
        /// 인덱스 이름 (선택적)
        name: Option<String>,
        /// 레이블
        label: String,
        /// 프로퍼티 목록
        properties: Vec<String>,
    },
    /// DROP INDEX ON :Label(property) 또는 DROP INDEX name
    Drop {
        /// 인덱스 이름 (있으면 이름으로 삭제)
        name: Option<String>,
        /// 레이블 (이름 없을 때)
        label: Option<String>,
        /// 프로퍼티 목록
        properties: Vec<String>,
    },
    /// SHOW INDEXES
    Show,
    /// CREATE SPATIAL INDEX ON :Label(property)
    CreateSpatial {
        /// 레이블
        label: String,
        /// 프로퍼티 (공간 좌표를 저장하는 프로퍼티)
        property: String,
    },
    /// DROP SPATIAL INDEX ON :Label(property)
    DropSpatial {
        /// 레이블
        label: String,
        /// 프로퍼티
        property: String,
    },
    /// SHOW SPATIAL INDEXES
    ShowSpatial,
    /// CREATE FULLTEXT INDEX name FOR (n:Label) ON EACH [n.property, ...] ANALYZER analyzer
    CreateFulltext {
        name: String,
        label: String,
        properties: Vec<String>,
        analyzer: String,
    },
    /// DROP FULLTEXT INDEX name
    DropFulltext { name: String },
    /// SHOW FULLTEXT INDEXES
    ShowFulltext,
    /// CREATE VECTOR INDEX name FOR (n:Label) ON (n.property) DIMENSIONS n DISTANCE metric
    CreateVector {
        name: String,
        label: String,
        property: String,
        dimensions: u32,
        distance: String,
    },
    /// DROP VECTOR INDEX name
    DropVector { name: String },
    /// SHOW VECTOR INDEXES
    ShowVector,
}
