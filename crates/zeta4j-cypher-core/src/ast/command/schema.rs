//! 스키마 관리 명령 AST
//!
//! CREATE/DROP NODE LABEL, CREATE/DROP RELATIONSHIP TYPE 등

use zeta4j_graph::core::Value;

/// 스키마 관리 명령
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaCommand {
    /// CREATE NODE LABEL :Label (prop TYPE, ...)
    CreateNodeLabel {
        /// 라벨 이름
        label: String,
        /// 프로퍼티 정의 목록
        properties: Vec<SchemaPropertyDef>,
    },
    /// CREATE RELATIONSHIP TYPE :TYPE (prop TYPE, ...) FROM :Label TO :Label
    CreateRelationshipType {
        /// 관계 타입 이름
        rel_type: String,
        /// 프로퍼티 정의 목록
        properties: Vec<SchemaPropertyDef>,
        /// 허용되는 소스 라벨들
        from_labels: Option<Vec<String>>,
        /// 허용되는 타겟 라벨들
        to_labels: Option<Vec<String>>,
    },
    /// DROP NODE LABEL :Label
    DropNodeLabel { label: String },
    /// DROP RELATIONSHIP TYPE :TYPE
    DropRelationshipType { rel_type: String },
    /// SHOW NODE LABELS
    ShowNodeLabels,
    /// SHOW RELATIONSHIP TYPES
    ShowRelationshipTypes,
    /// SHOW SCHEMA FOR :Label 또는 :RELATIONSHIP_TYPE
    ShowSchemaFor {
        /// true면 노드 라벨, false면 관계 타입
        is_node: bool,
        /// 라벨 또는 관계 타입 이름
        name: String,
    },
}

/// 스키마 프로퍼티 정의
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaPropertyDef {
    /// 프로퍼티 이름
    pub name: String,
    /// 프로퍼티 타입 문자열 (STRING, INTEGER, FLOAT, BOOLEAN, DATE, DATETIME 등)
    pub property_type: String,
    /// NULL 허용 여부 (false = NOT NULL)
    pub nullable: bool,
    /// 기본값
    pub default_value: Option<Value>,
}
