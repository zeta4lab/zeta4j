//! 읽기 절 AST
//!
//! MATCH, OPTIONAL MATCH, LOAD CSV 절

use super::super::expr::Expression;
use super::super::pattern::Pattern;

/// 읽기 절 - 그래프에서 데이터를 읽는 절
///
/// MATCH 또는 OPTIONAL MATCH를 통해 그래프 패턴을 검색합니다.
/// WHERE 조건으로 결과를 필터링할 수 있습니다.
///
/// # Variants
///
/// * `Match` - 필수 패턴 매칭 (매칭 실패 시 해당 행 제외)
///   - `optional: false` → 일반 MATCH
///   - `optional: true` → OPTIONAL MATCH (매칭 실패 시 NULL)
/// * `LoadCsv` - CSV 파일에서 데이터 로드
///
/// # 예제
///
/// ```text
/// // 필수 MATCH
/// MATCH (n:Person)-[:KNOWS]->(m)
/// WHERE n.age > 30
/// RETURN n, m
///
/// // OPTIONAL MATCH (친구가 없어도 n은 반환)
/// MATCH (n:Person)
/// OPTIONAL MATCH (n)-[:KNOWS]->(friend)
/// RETURN n, friend
///
/// // 다중 패턴 (카티션 곱)
/// MATCH (a:Person), (b:Company)
/// WHERE a.company = b.name
/// RETURN a, b
///
/// // LOAD CSV
/// LOAD CSV WITH HEADERS FROM 'file:///data.csv' AS row
/// CREATE (:Person {name: row.name})
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum ReadingClause {
    /// MATCH 또는 OPTIONAL MATCH 절
    Match {
        /// 매칭할 패턴들 (콤마로 분리된 여러 패턴 지원)
        /// 예: `MATCH (a), (b), (c)-[:REL]->(d)`
        patterns: Vec<Pattern>,
        /// OPTIONAL MATCH 여부 (true면 매칭 실패해도 NULL로 반환)
        optional: bool,
        /// WHERE 필터 조건 (선택적)
        where_clause: Option<Expression>,
    },
    /// LOAD CSV 절 - CSV 파일에서 데이터 로드
    ///
    /// # 예제
    ///
    /// ```text
    /// // 기본 사용 (배열로 접근)
    /// LOAD CSV FROM 'file:///data.csv' AS row
    /// CREATE (:Person {name: row[0], age: toInteger(row[1])})
    ///
    /// // 헤더 사용 (맵으로 접근)
    /// LOAD CSV WITH HEADERS FROM 'file:///data.csv' AS row
    /// CREATE (:Person {name: row.name, age: toInteger(row.age)})
    ///
    /// // 필드 구분자 지정
    /// LOAD CSV FROM 'file:///data.tsv' AS row FIELDTERMINATOR '\t'
    /// ```
    LoadCsv {
        /// WITH HEADERS 옵션 (true면 첫 행을 헤더로 사용, 맵으로 접근)
        with_headers: bool,
        /// CSV 파일 URL (file:// 또는 http://)
        url: String,
        /// 행 변수 이름 (AS 뒤의 변수)
        variable: String,
        /// 필드 구분자 (기본값: ',')
        field_terminator: Option<char>,
    },
}
