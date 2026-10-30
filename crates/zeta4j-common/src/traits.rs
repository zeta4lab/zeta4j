//! 공통 트레이트
//!
//! 모든 zeta4j 크레이트에서 사용하는 공통 트레이트입니다.

use crate::error::Result;
/// 상태 확인 가능한 컴포넌트
pub trait HealthCheck: Send + Sync {
    /// 헬스 체크
    fn health_check(&self) -> HealthStatus;
}

/// 헬스 상태
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// 정상
    Healthy,
    /// 성능 저하
    Degraded(String),
    /// 비정상
    Unhealthy(String),
}

impl HealthStatus {
    /// 정상 여부
    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy)
    }
}

/// 메트릭 수집 가능한 컴포넌트
pub trait MetricsCollector: Send + Sync {
    /// 메트릭 수집
    fn collect_metrics(&self) -> Vec<Metric>;
}

/// 메트릭 데이터
#[derive(Debug, Clone)]
pub struct Metric {
    /// 메트릭 이름
    pub name: String,
    /// 메트릭 값
    pub value: MetricValue,
    /// 라벨
    pub labels: Vec<(String, String)>,
}

/// 메트릭 값
#[derive(Debug, Clone)]
pub enum MetricValue {
    /// 카운터
    Counter(u64),
    /// 게이지
    Gauge(f64),
    /// 히스토그램
    Histogram(Vec<f64>),
}

/// 직렬화 가능한 컴포넌트
pub trait Persistable: Send + Sync {
    /// 바이트로 직렬화
    fn to_bytes(&self) -> Result<Vec<u8>>;

    /// 바이트에서 역직렬화
    fn from_bytes(bytes: &[u8]) -> Result<Self>
    where
        Self: Sized;
}

/// 검증 가능한 컴포넌트
pub trait Validatable {
    /// 유효성 검증
    fn validate(&self) -> Result<()>;
}
