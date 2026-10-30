//! Expression parsing
//!
//! 표현식 파싱: 리터럴, 변수, 속성 접근, 함수 호출, 비교, 논리 연산 등

use nom::{
    IResult,
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    character::complete::{char, multispace0, multispace1},
    combinator::{map, opt, value},
    multi::{many0, many1, separated_list0},
    sequence::{delimited, preceded, tuple},
};

use super::functions::scalar_function_from_name;
use super::patterns::parse_pattern;
use super::primitives::parse_value;
use super::utils::{identifier, keyword, ws};
use crate::ast::*;
use zeta4j_graph::core::Value;

/// 파라미터 파싱 ($param 또는 $1)
pub fn parse_parameter(input: &str) -> IResult<&str, Expression> {
    let (input, _) = char('$')(input)?;
    let (input, name) = identifier(input)?;
    Ok((input, Expression::Parameter(name)))
}

/// 속성 접근 파싱 (var.prop)
pub fn parse_property_access(input: &str) -> IResult<&str, Expression> {
    map(
        tuple((identifier, char('.'), identifier)),
        |(var, _, prop)| Expression::PropertyAccess(var, prop),
    )(input)
}

// =============================================================================
// 집계 함수 파서
// =============================================================================

/// 집계 함수 파서 팩토리
///
/// 동일한 패턴의 집계 함수 파서를 생성합니다:
/// `FUNC_NAME '(' [DISTINCT] expr ')'`
///
/// # 예시
///
/// ```ignore
/// let sum_parser = aggregate_parser("SUM", AggregateFunction::Sum);
/// let (_, expr) = sum_parser("SUM(n.age)")?;
/// ```
fn aggregate_parser(
    keyword: &'static str,
    func: AggregateFunction,
) -> impl FnMut(&str) -> IResult<&str, Expression> {
    move |input| {
        map(
            tuple((
                tag_no_case(keyword),
                multispace0,
                char('('),
                multispace0,
                opt(tuple((tag_no_case("DISTINCT"), multispace1))),
                parse_non_aggregate_expression,
                multispace0,
                char(')'),
            )),
            |(_, _, _, _, distinct, expr, _, _)| {
                Expression::Aggregate(AggregateFunctionCall {
                    function: func.clone(),
                    argument: Some(Box::new(expr)),
                    distinct: distinct.is_some(),
                })
            },
        )(input)
    }
}

/// 집계 함수 파서 (COUNT, SUM, AVG, MIN, MAX, COLLECT)
///
/// # 지원 문법
///
/// - `COUNT(*)` - 전체 행 수
/// - `COUNT(expr)` - NULL 제외 수
/// - `COUNT(DISTINCT expr)` - 중복 제거 수
/// - `SUM(expr)`, `AVG(expr)`, `MIN(expr)`, `MAX(expr)`, `COLLECT(expr)`
/// - 모든 집계 함수는 `DISTINCT` 옵션 지원
pub fn parse_aggregate_function(input: &str) -> IResult<&str, Expression> {
    // COUNT(*) - 특수 케이스 (인자 없음)
    let count_all = map(
        tuple((
            tag_no_case("COUNT"),
            multispace0,
            char('('),
            multispace0,
            char('*'),
            multispace0,
            char(')'),
        )),
        |_| {
            Expression::Aggregate(AggregateFunctionCall {
                function: AggregateFunction::CountAll,
                argument: None,
                distinct: false,
            })
        },
    );

    alt((
        count_all,
        aggregate_parser("COUNT", AggregateFunction::Count),
        aggregate_parser("SUM", AggregateFunction::Sum),
        aggregate_parser("AVG", AggregateFunction::Avg),
        aggregate_parser("MIN", AggregateFunction::Min),
        aggregate_parser("MAX", AggregateFunction::Max),
        aggregate_parser("COLLECT", AggregateFunction::Collect),
    ))(input)
}

/// 점으로 구분된 함수명 파서 (예: vector.similarity.cosine)
fn parse_dotted_function_name(input: &str) -> IResult<&str, String> {
    let (input, first) = identifier(input)?;
    let (input, rest) = many0(preceded(char('.'), identifier))(input)?;

    if rest.is_empty() {
        Ok((input, first))
    } else {
        let mut name = first;
        for part in rest {
            name.push('.');
            name.push_str(&part);
        }
        Ok((input, name))
    }
}

/// 스칼라 함수 파서 (toUpper, toLower, vector.similarity.cosine 등)
pub fn parse_scalar_function(input: &str) -> IResult<&str, Expression> {
    // 점으로 구분된 함수명 지원 (예: vector.similarity.cosine)
    let (input, name) = parse_dotted_function_name(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;

    let function = scalar_function_from_name(&name.to_lowercase()).ok_or_else(|| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag))
    })?;

    // 스칼라 함수 인수로 집계 함수도 허용 (예: round(avg(x)))
    let (input, args) = separated_list0(ws(char(',')), parse_or_expression)(input)?;

    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((
        input,
        Expression::FunctionCall(ScalarFunctionCall {
            function,
            arguments: args,
        }),
    ))
}

/// 리스트 표현식 파서 (일반 리스트 또는 리스트 컴프리헨션)
pub fn parse_list_expression(input: &str) -> IResult<&str, Expression> {
    // 먼저 리스트 컴프리헨션 시도
    if let Ok(result) = parse_list_comprehension(input) {
        return Ok(result);
    }

    // 일반 리스트 리터럴
    map(
        delimited(
            char('['),
            ws(separated_list0(ws(char(',')), parse_primary_expression)),
            char(']'),
        ),
        Expression::List,
    )(input)
}

/// 리스트 컴프리헨션 파서
/// [variable IN list_expr]
/// [variable IN list_expr | map_expr]
/// [variable IN list_expr WHERE filter_expr]
/// [variable IN list_expr WHERE filter_expr | map_expr]
fn parse_list_comprehension(input: &str) -> IResult<&str, Expression> {
    let (input, _) = char('[')(input)?;
    let (input, _) = multispace0(input)?;

    // variable IN list_expr 파싱
    let (input, variable) = identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("IN")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, list_expr) = parse_or_expression(input)?;
    let (input, _) = multispace0(input)?;

    // 선택적 WHERE 조건
    let (input, filter) = opt(preceded(
        tuple((tag_no_case("WHERE"), multispace1)),
        parse_or_expression,
    ))(input)?;
    let (input, _) = multispace0(input)?;

    // 선택적 | 변환 표현식
    let (input, map_expr) = opt(preceded(
        tuple((char('|'), multispace0)),
        parse_or_expression,
    ))(input)?;
    let (input, _) = multispace0(input)?;

    let (input, _) = char(']')(input)?;

    Ok((
        input,
        Expression::ListComprehension(ListComprehensionExpr {
            variable,
            list_expr: Box::new(list_expr),
            filter: filter.map(Box::new),
            map_expr: map_expr.map(Box::new),
        }),
    ))
}

/// 맵 표현식 파서 {key: value, ...}
pub fn parse_map_expression(input: &str) -> IResult<&str, Expression> {
    map(
        delimited(
            char('{'),
            ws(separated_list0(
                ws(char(',')),
                tuple((identifier, ws(char(':')), ws(parse_or_expression))),
            )),
            char('}'),
        ),
        |pairs| Expression::Map(pairs.into_iter().map(|(k, _, v)| (k, v)).collect()),
    )(input)
}

/// Phase 3: 벡터 리터럴 파서
/// vector([0.1, 0.2, 0.3]) 또는 vector($param)
pub fn parse_vector_literal(input: &str) -> IResult<&str, Expression> {
    let (input, _) = tag_no_case("vector")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;

    // 배열 또는 파라미터
    let (input, expr) = alt((
        // vector([1.0, 2.0, 3.0])
        map(parse_vector_array_literal, |elements| {
            Expression::Vector(VectorExpression::Literal(elements))
        }),
        // vector($param)
        map(parse_vector_parameter, |name| {
            Expression::Vector(VectorExpression::Parameter(name))
        }),
    ))(input)?;

    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;

    Ok((input, expr))
}

/// 벡터용 배열 리터럴 파싱: [0.1, 0.2, 0.3]
fn parse_vector_array_literal(input: &str) -> IResult<&str, Vec<Expression>> {
    delimited(
        tuple((char('['), multispace0)),
        separated_list0(
            tuple((multispace0, char(','), multispace0)),
            parse_vector_element,
        ),
        tuple((multispace0, char(']'))),
    )(input)
}

/// 벡터 요소 파싱 (숫자 리터럴, 파라미터, 음수 표현식)
fn parse_vector_element(input: &str) -> IResult<&str, Expression> {
    alt((
        // 음수: -1.5
        map(
            tuple((
                char('-'),
                multispace0,
                map(parse_value, Expression::Literal),
            )),
            |(_, _, expr)| {
                if let Expression::Literal(Value::Integer(i)) = expr {
                    Expression::Literal(Value::Integer(-i))
                } else if let Expression::Literal(Value::Float(f)) = expr {
                    Expression::Literal(Value::Float(-f))
                } else {
                    expr
                }
            },
        ),
        // 양수 숫자
        map(parse_value, Expression::Literal),
        // 파라미터
        parse_parameter,
    ))(input)
}

/// 벡터 파라미터 파싱: $embedding -> "embedding"
fn parse_vector_parameter(input: &str) -> IResult<&str, String> {
    let (input, _) = char('$')(input)?;
    let (input, name) = identifier(input)?;
    Ok((input, name))
}

/// CASE 표현식 파서
pub fn parse_case_expression(input: &str) -> IResult<&str, Expression> {
    let (input, _) = ws(tag_no_case("CASE"))(input)?;
    let (input, _) = multispace0(input)?;

    let (input, operand) = if input.to_uppercase().starts_with("WHEN") {
        (input, None)
    } else {
        let (input, op) = parse_or_expression(input)?;
        (input, Some(op))
    };

    let (input, when_clauses) = many1(parse_when_clause)(input)?;

    let (input, else_result) =
        opt(preceded(ws(tag_no_case("ELSE")), ws(parse_or_expression)))(input)?;

    let (input, _) = ws(tag_no_case("END"))(input)?;

    Ok((
        input,
        Expression::Case(CaseExpression {
            operand: operand.map(Box::new),
            when_clauses,
            else_result: else_result.map(Box::new),
        }),
    ))
}

/// WHEN ... THEN ... 절 파서
fn parse_when_clause(input: &str) -> IResult<&str, WhenClause> {
    let (input, _) = ws(tag_no_case("WHEN"))(input)?;
    let (input, condition) = ws(parse_or_expression)(input)?;
    let (input, _) = ws(tag_no_case("THEN"))(input)?;
    let (input, result) = ws(parse_or_expression)(input)?;

    Ok((input, WhenClause { condition, result }))
}

/// EXISTS 서브쿼리 파서
/// 지원 형식:
/// - EXISTS { (n)-[:REL]->(m) }
/// - EXISTS { MATCH (n)-[:REL]->(m) }
/// - EXISTS { MATCH (n)-[:REL]->(m) WHERE condition }
pub fn parse_exists_expression(input: &str) -> IResult<&str, Expression> {
    let (input, _) = ws(tag_no_case("EXISTS"))(input)?;
    let (input, _) = ws(char('{'))(input)?;

    // MATCH 키워드는 선택적
    let (input, _) = opt(ws(tag_no_case("MATCH")))(input)?;

    let (input, pattern) = ws(parse_pattern)(input)?;

    let (input, where_clause) =
        opt(preceded(ws(tag_no_case("WHERE")), ws(parse_or_expression)))(input)?;

    let (input, _) = ws(char('}'))(input)?;

    Ok((
        input,
        Expression::Exists(ExistsExpression {
            pattern,
            where_clause: where_clause.map(Box::new),
        }),
    ))
}

/// 비교 연산자 파서
pub fn parse_comparison_op(input: &str) -> IResult<&str, ComparisonOp> {
    ws(alt((
        value(ComparisonOp::LessEqual, nom::bytes::complete::tag("<=")),
        value(ComparisonOp::GreaterEqual, nom::bytes::complete::tag(">=")),
        value(ComparisonOp::NotEqual, nom::bytes::complete::tag("<>")),
        value(ComparisonOp::NotEqual, nom::bytes::complete::tag("!=")),
        value(ComparisonOp::Equal, char('=')),
        value(ComparisonOp::LessThan, char('<')),
        value(ComparisonOp::GreaterThan, char('>')),
        value(ComparisonOp::Contains, tag_no_case("CONTAINS")),
        value(
            ComparisonOp::StartsWith,
            preceded(
                tag_no_case("STARTS"),
                preceded(ws(tag_no_case("WITH")), |i| Ok((i, ()))),
            ),
        ),
        value(
            ComparisonOp::EndsWith,
            preceded(
                tag_no_case("ENDS"),
                preceded(ws(tag_no_case("WITH")), |i| Ok((i, ()))),
            ),
        ),
        value(ComparisonOp::In, tag_no_case("IN")),
    )))(input)
}

/// 집계 함수가 아닌 기본 표현식 (단일 원자 표현식)
fn parse_non_aggregate_primary(input: &str) -> IResult<&str, Expression> {
    alt((
        delimited(ws(char('(')), parse_or_expression, ws(char(')'))),
        parse_exists_expression,
        parse_case_expression,
        parse_vector_literal, // Phase 3: 벡터 리터럴 (scalar_function보다 먼저)
        parse_scalar_function,
        parse_list_expression,
        parse_map_expression,
        parse_parameter,
        map(parse_value, Expression::Literal),
        parse_property_access,
        map(identifier, Expression::Variable),
    ))(input)
}

/// 집계 함수가 아닌 곱셈/나눗셈 표현식
fn parse_non_aggregate_multiplicative(input: &str) -> IResult<&str, Expression> {
    let (input, first) = parse_non_aggregate_primary(input)?;
    let (input, rest) = many0(tuple((
        parse_multiplicative_op,
        parse_non_aggregate_primary,
    )))(input)?;

    let result = rest
        .into_iter()
        .fold(first, |acc, (op, right)| Expression::Arithmetic {
            left: Box::new(acc),
            op,
            right: Box::new(right),
        });

    Ok((input, result))
}

/// 집계 함수가 아닌 표현식 (집계 함수 인자용, 산술 연산 지원)
///
/// 스칼라 함수 인수에서 산술 연산을 지원합니다:
/// - `toString(i + 1)` - 덧셈
/// - `substring(str, 0, len * 2)` - 곱셈
pub fn parse_non_aggregate_expression(input: &str) -> IResult<&str, Expression> {
    let (input, first) = parse_non_aggregate_multiplicative(input)?;
    let (input, rest) = many0(tuple((
        parse_additive_op,
        parse_non_aggregate_multiplicative,
    )))(input)?;

    let result = rest
        .into_iter()
        .fold(first, |acc, (op, right)| Expression::Arithmetic {
            left: Box::new(acc),
            op,
            right: Box::new(right),
        });

    Ok((input, result))
}

/// 기본 표현식 (비교/논리 연산 없이)
fn parse_base_expression(input: &str) -> IResult<&str, Expression> {
    alt((
        delimited(ws(char('(')), parse_or_expression, ws(char(')'))),
        parse_exists_expression,
        parse_case_expression,
        parse_aggregate_function,
        parse_vector_literal, // Phase 3: 벡터 리터럴 (scalar_function보다 먼저)
        parse_scalar_function,
        parse_list_expression,
        parse_map_expression,
        parse_parameter,
        map(parse_value, Expression::Literal),
        parse_property_access,
        map(identifier, Expression::Variable),
    ))(input)
}

/// 인덱스 접근 종류 (단일 인덱스 vs 범위)
enum SubscriptAccess {
    /// 단일 인덱스: [index]
    Index(Expression),
    /// 범위 슬라이싱: [start..end]
    Range {
        start: Option<Expression>,
        end: Option<Expression>,
    },
}

/// 인덱스 접근 파싱 (expr[0], expr[0..10], expr[..10], expr[0..] 등)
fn parse_subscript_index(input: &str) -> IResult<&str, SubscriptAccess> {
    let (input, _) = tuple((multispace0, char('['), multispace0))(input)?;

    // 먼저 범위 표현식인지 확인 ([..end] 형태)
    if let Ok((remaining, _)) = tag::<_, _, nom::error::Error<&str>>("..")(input) {
        // [..end] 또는 [..]
        let (remaining, _) = multispace0(remaining)?;
        if let Ok((remaining, _)) = char::<_, nom::error::Error<&str>>(']')(remaining) {
            // [..] - 전체 범위
            return Ok((
                remaining,
                SubscriptAccess::Range {
                    start: None,
                    end: None,
                },
            ));
        }
        // [..end]
        let (remaining, end_expr) = parse_or_expression(remaining)?;
        let (remaining, _) = tuple((multispace0, char(']')))(remaining)?;
        return Ok((
            remaining,
            SubscriptAccess::Range {
                start: None,
                end: Some(end_expr),
            },
        ));
    }

    // 시작 표현식 파싱
    let (input, start_expr) = parse_or_expression(input)?;
    let (input, _) = multispace0(input)?;

    // 범위 연산자 확인
    if let Ok((remaining, _)) = tag::<_, _, nom::error::Error<&str>>("..")(input) {
        let (remaining, _) = multispace0(remaining)?;
        // [start..] 또는 [start..end]
        if let Ok((remaining, _)) = char::<_, nom::error::Error<&str>>(']')(remaining) {
            // [start..] - 끝까지
            return Ok((
                remaining,
                SubscriptAccess::Range {
                    start: Some(start_expr),
                    end: None,
                },
            ));
        }
        // [start..end]
        let (remaining, end_expr) = parse_or_expression(remaining)?;
        let (remaining, _) = tuple((multispace0, char(']')))(remaining)?;
        return Ok((
            remaining,
            SubscriptAccess::Range {
                start: Some(start_expr),
                end: Some(end_expr),
            },
        ));
    }

    // 단일 인덱스
    let (input, _) = char(']')(input)?;
    Ok((input, SubscriptAccess::Index(start_expr)))
}

/// 기본 표현식 + 선택적 인덱스 접근
pub fn parse_primary_expression(input: &str) -> IResult<&str, Expression> {
    let (input, base) = parse_base_expression(input)?;
    let (input, subscripts) = many0(parse_subscript_index)(input)?;

    // 인덱스 접근이 있으면 중첩된 Subscript/SubscriptRange 표현식으로 변환
    let result = subscripts
        .into_iter()
        .fold(base, |acc, access| match access {
            SubscriptAccess::Index(index) => Expression::Subscript {
                expr: Box::new(acc),
                index: Box::new(index),
            },
            SubscriptAccess::Range { start, end } => Expression::SubscriptRange {
                expr: Box::new(acc),
                start: start.map(Box::new),
                end: end.map(Box::new),
            },
        });

    Ok((input, result))
}

/// 거듭제곱 표현식 (^) - 곱셈보다 높은 우선순위
fn parse_power_expression(input: &str) -> IResult<&str, Expression> {
    let (input, first) = parse_primary_expression(input)?;
    let (input, rest) = many0(tuple((ws(char('^')), parse_primary_expression)))(input)?;

    // 거듭제곱은 오른쪽 결합 (2^3^2 = 2^(3^2) = 2^9 = 512)
    if rest.is_empty() {
        return Ok((input, first));
    }

    let mut exprs: Vec<Expression> = vec![first];
    for (_, expr) in rest {
        exprs.push(expr);
    }

    // 오른쪽부터 결합
    let first_from_right = exprs.remove(exprs.len() - 1);
    let result =
        exprs
            .into_iter()
            .rev()
            .fold(first_from_right, |acc, left| Expression::Arithmetic {
                left: Box::new(left),
                op: ArithmeticOp::Power,
                right: Box::new(acc),
            });

    Ok((input, result))
}

/// 곱셈/나눗셈/나머지 연산자 파서
fn parse_multiplicative_op(input: &str) -> IResult<&str, ArithmeticOp> {
    ws(alt((
        value(ArithmeticOp::Multiply, char('*')),
        value(ArithmeticOp::Divide, char('/')),
        value(ArithmeticOp::Modulo, char('%')),
    )))(input)
}

/// 곱셈/나눗셈/나머지 표현식 (*, /, %)
fn parse_multiplicative_expression(input: &str) -> IResult<&str, Expression> {
    let (input, first) = parse_power_expression(input)?;
    let (input, rest) = many0(tuple((parse_multiplicative_op, parse_power_expression)))(input)?;

    let result = rest
        .into_iter()
        .fold(first, |acc, (op, right)| Expression::Arithmetic {
            left: Box::new(acc),
            op,
            right: Box::new(right),
        });

    Ok((input, result))
}

/// 덧셈/뺄셈 연산자 파서
fn parse_additive_op(input: &str) -> IResult<&str, ArithmeticOp> {
    ws(alt((
        value(ArithmeticOp::Add, char('+')),
        value(ArithmeticOp::Subtract, char('-')),
    )))(input)
}

/// 덧셈/뺄셈 표현식 (+, -)
fn parse_additive_expression(input: &str) -> IResult<&str, Expression> {
    let (input, first) = parse_multiplicative_expression(input)?;
    let (input, rest) = many0(tuple((parse_additive_op, parse_multiplicative_expression)))(input)?;

    let result = rest
        .into_iter()
        .fold(first, |acc, (op, right)| Expression::Arithmetic {
            left: Box::new(acc),
            op,
            right: Box::new(right),
        });

    Ok((input, result))
}

/// IS NULL / IS NOT NULL 파서
fn parse_is_null_expression(input: &str) -> IResult<&str, Expression> {
    let (input, expr) = parse_additive_expression(input)?;
    let (input, is_null) = opt(ws(tuple((
        tag_no_case("IS"),
        opt(ws(tag_no_case("NOT"))),
        ws(tag_no_case("NULL")),
    ))))(input)?;

    if let Some((_, not_opt, _)) = is_null {
        Ok((
            input,
            Expression::IsNull {
                expr: Box::new(expr),
                negated: not_opt.is_some(),
            },
        ))
    } else {
        Ok((input, expr))
    }
}

/// 비교 표현식 (a = b, a < b 등)
fn parse_comparison_expression(input: &str) -> IResult<&str, Expression> {
    let (input, left) = parse_is_null_expression(input)?;
    let (input, rest) = opt(tuple((parse_comparison_op, parse_is_null_expression)))(input)?;

    if let Some((op, right)) = rest {
        Ok((
            input,
            Expression::Comparison {
                left: Box::new(left),
                op,
                right: Box::new(right),
            },
        ))
    } else {
        Ok((input, left))
    }
}

/// NOT 표현식
fn parse_not_expression(input: &str) -> IResult<&str, Expression> {
    let (input, not_opt) = opt(ws(tag_no_case("NOT")))(input)?;
    let (input, expr) = parse_comparison_expression(input)?;

    if not_opt.is_some() {
        Ok((
            input,
            Expression::Logical {
                left: None,
                op: LogicalOp::Not,
                right: Box::new(expr),
            },
        ))
    } else {
        Ok((input, expr))
    }
}

/// AND 표현식
pub fn parse_and_expression(input: &str) -> IResult<&str, Expression> {
    let (input, first) = parse_not_expression(input)?;
    let (input, rest) = many0(preceded(ws(keyword("AND")), parse_not_expression))(input)?;

    let result = rest
        .into_iter()
        .fold(first, |acc, expr| Expression::Logical {
            left: Some(Box::new(acc)),
            op: LogicalOp::And,
            right: Box::new(expr),
        });

    Ok((input, result))
}

/// OR 표현식 (최상위 우선순위)
pub fn parse_or_expression(input: &str) -> IResult<&str, Expression> {
    let (input, first) = parse_and_expression(input)?;
    let (input, rest) = many0(preceded(ws(keyword("OR")), parse_and_expression))(input)?;

    let result = rest
        .into_iter()
        .fold(first, |acc, expr| Expression::Logical {
            left: Some(Box::new(acc)),
            op: LogicalOp::Or,
            right: Box::new(expr),
        });

    Ok((input, result))
}

/// 표현식 파서 (최상위)
pub fn parse_expression(input: &str) -> IResult<&str, Expression> {
    parse_or_expression(input)
}
