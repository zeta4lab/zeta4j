//! 연산자 AST
//!
//! 비교, 논리, 산술 연산자

/// 비교 연산자 - 두 값을 비교하는 연산
///
/// # 예제
///
/// ```text
/// n.age > 30                     // GreaterThan
/// n.name = 'Alice'               // Equal
/// n.email CONTAINS '@gmail'      // Contains
/// n.status IN ['active', 'new']  // In
/// n.deleted IS NULL              // IsNull
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum ComparisonOp {
    /// 같음 (`=`)
    Equal,
    /// 다름 (`<>` 또는 `!=`)
    NotEqual,
    /// 작음 (`<`)
    LessThan,
    /// 작거나 같음 (`<=`)
    LessEqual,
    /// 큼 (`>`)
    GreaterThan,
    /// 크거나 같음 (`>=`)
    GreaterEqual,
    /// 부분 문자열 포함 (`CONTAINS`)
    Contains,
    /// 접두사 일치 (`STARTS WITH`)
    StartsWith,
    /// 접미사 일치 (`ENDS WITH`)
    EndsWith,
    /// 리스트 내 포함 (`IN`)
    In,
    /// NULL 여부 (`IS NULL`)
    IsNull,
    /// NOT NULL 여부 (`IS NOT NULL`)
    IsNotNull,
}

/// 논리 연산자 - WHERE 조건을 결합하는 연산
///
/// 여러 조건을 논리적으로 결합하여 복합 조건을 구성합니다.
///
/// # Variants
///
/// * `And` (`AND`) - 양쪽 모두 참이면 참
/// * `Or` (`OR`) - 한쪽이라도 참이면 참
/// * `Not` (`NOT`) - 부정 (단항 연산)
/// * `Xor` (`XOR`) - 배타적 OR (한쪽만 참일 때 참)
///
/// # 우선순위
///
/// `NOT` > `AND` > `XOR` > `OR`
#[derive(Debug, Clone, PartialEq)]
pub enum LogicalOp {
    /// 논리 AND - 양쪽 모두 참이면 참
    And,
    /// 논리 OR - 한쪽이라도 참이면 참
    Or,
    /// 논리 NOT - 부정 (단항 연산)
    Not,
    /// 논리 XOR - 배타적 OR (한쪽만 참일 때 참)
    Xor,
}

/// 산술 연산자 - 숫자 값의 수학 연산
///
/// # 우선순위 (높은 것이 먼저 평가)
///
/// 1. 곱셈, 나눗셈, 나머지 (*, /, %)
/// 2. 덧셈, 뺄셈 (+, -)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticOp {
    /// 덧셈 (+)
    Add,
    /// 뺄셈 (-)
    Subtract,
    /// 곱셈 (*)
    Multiply,
    /// 나눗셈 (/)
    Divide,
    /// 나머지 (%)
    Modulo,
    /// 거듭제곱 (^)
    Power,
}
