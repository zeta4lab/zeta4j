// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 프로젝션 절 AST
//!
//! WITH, RETURN, UNWIND 절

use super::super::expr::Expression;

/// WITH 절 (중간 결과 전달 및 변환)
///
/// Cypher 쿼리에서 중간 결과를 다음 절로 전달하며, 변환/필터링/정렬을 수행합니다.
/// MATCH와 RETURN 사이에서 파이프라인 역할을 합니다.
///
/// # 주요 기능
///
/// * **변수 변환**: `WITH n.name AS name` - 새 변수로 매핑
/// * **집계**: `WITH count(n) AS cnt` - 집계 결과 전달
/// * **필터링**: `WITH n WHERE n.age > 30` - 조건 필터링
/// * **정렬/제한**: `WITH n ORDER BY n.age LIMIT 10` - 정렬 후 상위 n개
///
/// # 예제
///
/// ```text
/// // 집계 후 필터링
/// MATCH (n:Person)
/// WITH n.city AS city, count(n) AS cnt
/// WHERE cnt > 100
/// RETURN city, cnt
///
/// // 정렬 후 상위 10개
/// MATCH (n:Person)
/// WITH n ORDER BY n.score DESC LIMIT 10
/// RETURN n.name
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct WithClause {
    /// DISTINCT 여부 - 중복 제거
    pub distinct: bool,
    /// 반환할 항목들 (변수, 표현식, 별칭)
    pub items: Vec<ReturnItem>,
    /// ORDER BY 정렬 조건
    pub order_by: Option<Vec<OrderItem>>,
    /// SKIP 건너뛸 행 수
    pub skip: Option<u64>,
    /// LIMIT 최대 행 수
    pub limit: Option<u64>,
    /// WHERE 필터 조건
    pub where_clause: Option<Expression>,
}

/// RETURN 절 - 쿼리의 최종 결과를 반환
///
/// 쿼리 파이프라인의 마지막 단계로, 최종 결과를 구성합니다.
/// 정렬, 건너뛰기, 제한, 중복 제거 옵션을 포함할 수 있습니다.
///
/// # 주요 기능
///
/// * **항목 선택**: `RETURN n, m.name` - 반환할 변수/속성 지정
/// * **별칭**: `RETURN n.name AS name` - 결과 컬럼 이름 지정
/// * **모든 변수**: `RETURN *` - 스코프 내 모든 변수 반환
/// * **중복 제거**: `RETURN DISTINCT n.city` - 고유값만 반환
/// * **정렬**: `RETURN n ORDER BY n.age DESC` - 결과 정렬
/// * **페이징**: `RETURN n SKIP 10 LIMIT 5` - 페이지네이션
///
/// # 예제
///
/// ```text
/// // 기본 반환
/// MATCH (n:Person) RETURN n.name, n.age
///
/// // 별칭 사용
/// MATCH (n:Person) RETURN n.name AS name, n.age AS age
///
/// // 정렬 및 제한
/// MATCH (n:Person)
/// RETURN n.name, n.salary
/// ORDER BY n.salary DESC
/// SKIP 10 LIMIT 5
///
/// // 중복 제거
/// MATCH (n:Person) RETURN DISTINCT n.city
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnClause {
    /// 반환할 항목들 (Star 또는 Expression)
    pub items: Vec<ReturnItem>,
    /// DISTINCT 여부 - 중복 제거
    pub distinct: bool,
    /// ORDER BY 정렬 조건
    pub order_by: Option<Vec<OrderItem>>,
    /// SKIP 건너뛸 행 수
    pub skip: Option<u64>,
    /// LIMIT 최대 행 수
    pub limit: Option<u64>,
}

/// RETURN/WITH 절에서 반환할 개별 항목
///
/// # Variants
///
/// * `Star` - 모든 변수 반환 (`RETURN *`)
/// * `Expression` - 특정 표현식 반환 (변수, 속성, 함수 호출 등)
///
/// # 예제
///
/// ```text
/// RETURN *                    // Star
/// RETURN n                    // Expression { expr: Variable("n"), alias: None }
/// RETURN n.name AS name       // Expression { expr: PropertyAccess, alias: Some("name") }
/// RETURN count(n) AS cnt      // Expression { expr: Aggregate, alias: Some("cnt") }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum ReturnItem {
    /// 모든 스코프 내 변수 반환 (`RETURN *`)
    Star,
    /// 특정 표현식 반환 (변수, 속성, 함수 호출 등)
    Expression {
        /// 반환할 표현식
        expr: Expression,
        /// 결과 컬럼의 별칭 (AS name)
        alias: Option<String>,
    },
}

/// 정렬 항목 - ORDER BY 절의 개별 정렬 조건
///
/// # 예제
///
/// ```text
/// ORDER BY n.age              // ascending: true (기본값)
/// ORDER BY n.age ASC          // ascending: true
/// ORDER BY n.age DESC         // ascending: false
/// ORDER BY n.salary DESC, n.name ASC  // 복합 정렬
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem {
    /// 정렬 기준 표현식
    pub expr: Expression,
    /// 오름차순 여부 (true: ASC, false: DESC)
    pub ascending: bool,
}

/// UNWIND 절 (리스트를 행으로 펼침)
///
/// 리스트의 각 요소를 별도의 행으로 변환합니다.
///
/// # 예제
///
/// ```text
/// // 리스트 펼침
/// UNWIND [1, 2, 3] AS x
/// RETURN x
/// // 결과: 1, 2, 3 (각각 별도 행)
///
/// // 속성 배열 펼침
/// MATCH (n:Person)
/// UNWIND n.emails AS email
/// RETURN n.name, email
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct UnwindClause {
    /// 펼칠 표현식 (리스트)
    pub expr: Expression,
    /// 바인딩할 변수명
    pub alias: String,
}

/// LET 절 (변수 바인딩) - Neo4j 5.x
///
/// WITH 없이 변수를 표현식에 바인딩합니다.
/// 쿼리 가독성을 향상시키기 위한 간소화된 구문입니다.
///
/// # 예제
///
/// ```text
/// LET threshold = 100
/// MATCH (n:Product)
/// WHERE n.price > threshold
/// RETURN n
///
/// LET now = datetime()
/// CREATE (e:Event {createdAt: now})
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct LetClause {
    /// 바인딩할 변수명
    pub variable: String,
    /// 바인딩할 표현식
    pub expression: Expression,
}

/// USE 절 (데이터베이스 선택)
///
/// 쿼리를 실행할 데이터베이스를 지정합니다.
/// 멀티 데이터베이스 환경에서 사용됩니다.
///
/// # 예제
///
/// ```text
/// // 정적 데이터베이스 선택
/// USE mydb
/// MATCH (n) RETURN n
///
/// // 동적 데이터베이스 선택
/// USE graph.byName($dbName)
/// MATCH (n:Person) RETURN n
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct UseClause {
    /// 데이터베이스 이름 또는 동적 표현식
    pub database: DatabaseTarget,
}

/// 데이터베이스 대상
#[derive(Debug, Clone, PartialEq)]
pub enum DatabaseTarget {
    /// 정적 데이터베이스 이름
    Named(String),
    /// 동적 데이터베이스 선택 (graph.byName(...))
    Dynamic(Expression),
}
