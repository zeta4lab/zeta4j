//! 실행 계획 노드 타입
//!
//! 쿼리 실행 계획의 연산자 종류

/// 실행 계획 노드 타입
#[derive(Debug, Clone, PartialEq)]
pub enum PlanNodeType {
    // === 스캔 연산 ===
    /// 모든 노드 스캔
    AllNodesScan,
    /// 레이블로 노드 스캔
    NodeByLabelScan,
    /// ID로 노드 직접 조회
    NodeByIdSeek,
    /// 인덱스 스캔 (등호 조건)
    NodeByIndexScan,
    /// 인덱스를 사용한 범위 스캔 (비교 연산자)
    NodeByIndexRangeScan,
    /// 여러 인덱스 결과의 교집합
    IndexIntersection,

    // === 확장 연산 ===
    /// 관계 탐색
    Expand,
    /// OPTIONAL MATCH
    OptionalExpand,

    // === 필터 연산 ===
    /// WHERE 조건
    Filter,

    // === 집계 연산 ===
    /// COUNT, SUM 등
    Aggregation,

    // === 정렬/제한 연산 ===
    /// ORDER BY
    Sort,
    /// SKIP
    Skip,
    /// LIMIT
    Limit,
    /// DISTINCT
    Distinct,

    // === 결합 연산 ===
    /// UNION
    Union,
    /// UNION ALL
    UnionAll,
    /// 카티션 곱 (다중 MATCH 패턴)
    CartesianProduct,

    // === 생성/수정 연산 ===
    /// CREATE
    Create,
    /// DELETE
    Delete,
    /// SET n.prop = value (개별 속성 설정)
    SetProperty,
    /// SET n = {props} (전체 속성 교체)
    SetNodeProperties,
    /// SET n += {props} (속성 병합)
    SetNodePropertiesMerge,
    /// SET n:Label (레이블 추가)
    SetLabels,
    /// REMOVE n.prop (속성 제거)
    RemoveProperty,
    /// REMOVE n:Label (레이블 제거)
    RemoveLabels,
    /// MERGE
    Merge,
    /// FOREACH (리스트 반복 업데이트)
    Foreach,
    /// LOAD CSV (CSV 파일 로드)
    LoadCsv,

    // === 관리 연산 ===
    /// 관리 명령 (CREATE USER, CREATE DATABASE 등)
    Admin,

    // === 기타 ===
    /// RETURN/WITH 프로젝션
    Projection,
    /// UNWIND
    Unwind,
    /// CALL 서브쿼리
    Call,
    /// 최종 결과 생성
    ProduceResults,
    /// 빈 결과 (FINISH)
    EmptyResult,
}

impl std::fmt::Display for PlanNodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanNodeType::AllNodesScan => write!(f, "AllNodesScan"),
            PlanNodeType::NodeByLabelScan => write!(f, "NodeByLabelScan"),
            PlanNodeType::NodeByIdSeek => write!(f, "NodeByIdSeek"),
            PlanNodeType::NodeByIndexScan => write!(f, "NodeByIndexScan"),
            PlanNodeType::NodeByIndexRangeScan => write!(f, "NodeByIndexRangeScan"),
            PlanNodeType::IndexIntersection => write!(f, "IndexIntersection"),
            PlanNodeType::Expand => write!(f, "Expand"),
            PlanNodeType::OptionalExpand => write!(f, "OptionalExpand"),
            PlanNodeType::Filter => write!(f, "Filter"),
            PlanNodeType::Aggregation => write!(f, "Aggregation"),
            PlanNodeType::Sort => write!(f, "Sort"),
            PlanNodeType::Skip => write!(f, "Skip"),
            PlanNodeType::Limit => write!(f, "Limit"),
            PlanNodeType::Distinct => write!(f, "Distinct"),
            PlanNodeType::Union => write!(f, "Union"),
            PlanNodeType::UnionAll => write!(f, "UnionAll"),
            PlanNodeType::CartesianProduct => write!(f, "CartesianProduct"),
            PlanNodeType::Create => write!(f, "Create"),
            PlanNodeType::Delete => write!(f, "Delete"),
            PlanNodeType::SetProperty => write!(f, "SetProperty"),
            PlanNodeType::SetNodeProperties => write!(f, "SetNodeProperties"),
            PlanNodeType::SetNodePropertiesMerge => write!(f, "SetNodePropertiesMerge"),
            PlanNodeType::SetLabels => write!(f, "SetLabels"),
            PlanNodeType::RemoveProperty => write!(f, "RemoveProperty"),
            PlanNodeType::RemoveLabels => write!(f, "RemoveLabels"),
            PlanNodeType::Merge => write!(f, "Merge"),
            PlanNodeType::Foreach => write!(f, "Foreach"),
            PlanNodeType::LoadCsv => write!(f, "LoadCsv"),
            PlanNodeType::Admin => write!(f, "Admin"),
            PlanNodeType::Projection => write!(f, "Projection"),
            PlanNodeType::Unwind => write!(f, "Unwind"),
            PlanNodeType::Call => write!(f, "Call"),
            PlanNodeType::ProduceResults => write!(f, "ProduceResults"),
            PlanNodeType::EmptyResult => write!(f, "EmptyResult"),
        }
    }
}
