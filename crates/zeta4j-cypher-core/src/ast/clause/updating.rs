// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 업데이트 절 AST
//!
//! CREATE, DELETE, SET, MERGE, REMOVE 절

use super::super::expr::Expression;
use super::super::pattern::Pattern;

/// 쓰기 절 - 그래프 데이터를 변경하는 절
///
/// CREATE, DELETE, SET, MERGE를 통해 노드/관계를 생성, 삭제, 수정합니다.
///
/// # Variants
///
/// * `Create` - 새 노드/관계 생성
/// * `Delete` - 노드/관계 삭제 (DETACH DELETE로 관계 포함 삭제)
/// * `Set` - 프로퍼티 설정 또는 레이블 추가
/// * `Merge` - 존재하면 매칭, 없으면 생성 (Upsert 패턴)
///
/// # 예제
///
/// ```text
/// // CREATE - 새 노드 생성
/// CREATE (n:Person {name: 'Alice', age: 30})
///
/// // DELETE - 노드 삭제
/// MATCH (n:Person {name: 'Bob'})
/// DELETE n
///
/// // DETACH DELETE - 관계 포함 삭제
/// MATCH (n:Person {name: 'Bob'})
/// DETACH DELETE n
///
/// // SET - 프로퍼티 설정
/// MATCH (n:Person)
/// SET n.updated = timestamp()
///
/// // MERGE - Upsert 패턴
/// MERGE (n:Person {email: 'alice@example.com'})
/// ON CREATE SET n.created = timestamp()
/// ON MATCH SET n.lastLogin = timestamp()
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum UpdatingClause {
    /// 새 노드/관계 생성 (CREATE)
    Create(Pattern),
    /// 노드/관계 삭제 (DELETE / DETACH DELETE)
    Delete {
        /// 삭제할 변수들
        variables: Vec<String>,
        /// DETACH 여부 (true면 연결된 관계도 함께 삭제)
        detach: bool,
    },
    /// 프로퍼티/레이블 설정 (SET)
    Set {
        /// 설정할 항목들
        items: Vec<SetItem>,
    },
    /// 조건부 생성/수정 (MERGE)
    Merge {
        /// 매칭/생성할 패턴
        pattern: Pattern,
        /// ON CREATE SET - 새로 생성될 때만 실행
        on_create: Option<Vec<SetItem>>,
        /// ON MATCH SET - 기존 데이터 매칭될 때만 실행
        on_match: Option<Vec<SetItem>>,
    },
    /// 프로퍼티/레이블 제거 (REMOVE)
    Remove {
        /// 제거할 항목들
        items: Vec<RemoveItem>,
    },
    /// 리스트 반복 처리 (FOREACH)
    ///
    /// 리스트의 각 요소에 대해 업데이트 작업을 수행합니다.
    ///
    /// # 예제
    ///
    /// ```text
    /// // 리스트의 각 요소에 대해 노드 생성
    /// FOREACH (name IN ['Alice', 'Bob', 'Carol'] |
    ///   CREATE (:Person {name: name})
    /// )
    ///
    /// // 경로의 모든 노드에 프로퍼티 설정
    /// MATCH p = (a)-[*]->(b)
    /// FOREACH (n IN nodes(p) | SET n.visited = true)
    /// ```
    Foreach {
        /// 반복 변수 이름
        variable: String,
        /// 반복할 리스트 표현식
        list: Expression,
        /// 각 요소에 대해 실행할 업데이트 절들
        updates: Vec<UpdatingClause>,
    },
}

/// SET 절의 개별 설정 항목
///
/// 노드/관계의 프로퍼티를 설정하거나 노드에 레이블을 추가합니다.
///
/// # Variants
///
/// * `Property` - 개별 속성 설정: `SET n.prop = value`
/// * `AllProperties` - 속성 전체 교체: `SET n = {props}` (기존 속성 제거)
/// * `MergeProperties` - 속성 병합: `SET n += {props}` (기존 속성 유지)
/// * `AddLabel` - 레이블 추가: `SET n:Label1:Label2`
///
/// # 예제
///
/// ```text
/// // 개별 속성 설정
/// SET n.name = 'Alice', n.age = 30
///
/// // 속성 전체 교체 (기존 속성 제거됨)
/// SET n = {name: 'Bob', email: 'bob@example.com'}
///
/// // 속성 병합 (기존 속성 유지, 새 속성 추가/업데이트)
/// SET n += {updated: timestamp(), version: 2}
///
/// // 레이블 추가
/// SET n:Admin:Verified
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum SetItem {
    /// 개별 속성 설정: `SET n.prop = value`
    Property {
        /// 대상 변수 (노드 또는 관계)
        variable: String,
        /// 설정할 속성 이름
        property: String,
        /// 설정할 값 표현식
        value: Expression,
    },
    /// 속성 전체 교체: `SET n = {props}`
    /// 기존 속성이 모두 제거되고 새 속성으로 교체됨
    AllProperties {
        /// 대상 변수
        variable: String,
        /// 새 속성 맵 표현식
        properties: Expression,
    },
    /// 속성 병합: `SET n += {props}`
    /// 기존 속성을 유지하면서 새 속성 추가/업데이트
    MergeProperties {
        /// 대상 변수
        variable: String,
        /// 병합할 속성 맵 표현식
        properties: Expression,
    },
    /// 레이블 추가: `SET n:Label1:Label2`
    AddLabel {
        /// 대상 노드 변수
        variable: String,
        /// 추가할 레이블들
        labels: Vec<String>,
    },
}

/// REMOVE 절의 개별 제거 항목
///
/// 노드/관계의 프로퍼티를 제거하거나 노드에서 레이블을 제거합니다.
///
/// # Variants
///
/// * `Property` - 속성 제거: `REMOVE n.prop`
/// * `Label` - 레이블 제거: `REMOVE n:Label1:Label2`
///
/// # 예제
///
/// ```text
/// // 속성 제거
/// REMOVE n.tempField, n.deprecated
///
/// // 레이블 제거
/// REMOVE n:Temporary:Unverified
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum RemoveItem {
    /// 속성 제거: `REMOVE n.prop`
    Property {
        /// 대상 변수 (노드 또는 관계)
        variable: String,
        /// 제거할 속성 이름
        property: String,
    },
    /// 레이블 제거: `REMOVE n:Label1:Label2`
    Label {
        /// 대상 노드 변수
        variable: String,
        /// 제거할 레이블들
        labels: Vec<String>,
    },
}
