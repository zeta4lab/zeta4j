// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! M7.2: Health Check System
//!
//! 컴포넌트 상태 체크, Liveness/Readiness 프로브
//!
//! # 향상된 기능 (v1.5.0)
//!
//! - 실제 디스크/메모리/CPU 사용량 측정
//! - 상세한 시스템 메트릭 제공
//! - 연결 풀, 캐시 등 컴포넌트 헬스체크

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use sysinfo::{Disks, System};

// ============================================================================
// HealthStatus - 상태 정의
// ============================================================================

/// 헬스 상태
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "reason")]
#[derive(Default)]
pub enum HealthStatus {
    /// 정상 상태
    #[default]
    Healthy,
    /// 저하된 상태 (일부 기능 제한)
    Degraded(String),
    /// 비정상 상태
    Unhealthy(String),
}

impl HealthStatus {
    /// 정상 여부
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    /// 서비스 가능 여부 (Healthy 또는 Degraded)
    pub fn is_available(&self) -> bool {
        !matches!(self, HealthStatus::Unhealthy(_))
    }

    /// HTTP 상태 코드 반환
    pub fn http_status_code(&self) -> u16 {
        match self {
            HealthStatus::Healthy => 200,
            HealthStatus::Degraded(_) => 200,
            HealthStatus::Unhealthy(_) => 503,
        }
    }
}

// ============================================================================
// ComponentHealth - 컴포넌트 상태 트레이트
// ============================================================================

/// 컴포넌트 헬스 체크 트레이트
pub trait ComponentHealth: Send + Sync {
    /// 컴포넌트 이름
    fn name(&self) -> &str;

    /// 상태 체크
    fn check(&self) -> HealthStatus;

    /// 상세 정보
    fn details(&self) -> HashMap<String, String> {
        HashMap::new()
    }
}

// ============================================================================
// HealthChecker - 통합 헬스 체커
// ============================================================================

/// 통합 헬스 체커
pub struct HealthChecker {
    /// 등록된 컴포넌트들
    components: RwLock<Vec<Arc<dyn ComponentHealth>>>,
    /// 마지막 체크 시간
    last_check: RwLock<Option<Instant>>,
    /// 캐시된 상태
    cached_status: RwLock<Option<HealthCheckResult>>,
    /// 캐시 유효 시간
    cache_ttl: Duration,
}

impl HealthChecker {
    /// 새 헬스 체커 생성
    pub fn new() -> Self {
        Self {
            components: RwLock::new(Vec::new()),
            last_check: RwLock::new(None),
            cached_status: RwLock::new(None),
            cache_ttl: Duration::from_secs(5),
        }
    }

    /// 캐시 TTL 설정
    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// 컴포넌트 등록
    pub fn register<C: ComponentHealth + 'static>(&self, component: Arc<C>) {
        let mut components = self.components.write().unwrap_or_else(|e| e.into_inner());
        components.push(component);
    }

    /// 모든 컴포넌트 상태 체크
    pub fn check_all(&self) -> HealthCheckResult {
        // 캐시 확인
        {
            let last_check = self.last_check.read().unwrap_or_else(|e| e.into_inner());
            let cached = self.cached_status.read().unwrap_or_else(|e| e.into_inner());

            if let (Some(last), Some(cached)) = (last_check.as_ref(), cached.as_ref())
                && last.elapsed() < self.cache_ttl
            {
                return cached.clone();
            }
        }

        // 새로 체크
        let components = self.components.read().unwrap_or_else(|e| e.into_inner());
        let mut component_statuses = HashMap::new();
        let mut overall_status = HealthStatus::Healthy;

        for component in components.iter() {
            let status = component.check();
            let name = component.name().to_string();

            // 전체 상태 업데이트
            match (&overall_status, &status) {
                (HealthStatus::Healthy, HealthStatus::Degraded(reason)) => {
                    overall_status = HealthStatus::Degraded(reason.clone());
                }
                (_, HealthStatus::Unhealthy(reason)) => {
                    overall_status = HealthStatus::Unhealthy(reason.clone());
                }
                _ => {}
            }

            component_statuses.insert(
                name,
                ComponentStatus {
                    status,
                    details: component.details(),
                },
            );
        }

        let result = HealthCheckResult {
            status: overall_status,
            components: component_statuses,
            timestamp: chrono::Utc::now(),
        };

        // 캐시 업데이트
        {
            let mut last_check = self.last_check.write().unwrap_or_else(|e| e.into_inner());
            let mut cached = self
                .cached_status
                .write()
                .unwrap_or_else(|e| e.into_inner());
            *last_check = Some(Instant::now());
            *cached = Some(result.clone());
        }

        result
    }

    /// 특정 컴포넌트 상태 체크
    pub fn check_component(&self, name: &str) -> Option<ComponentStatus> {
        let components = self.components.read().unwrap_or_else(|e| e.into_inner());

        for component in components.iter() {
            if component.name() == name {
                return Some(ComponentStatus {
                    status: component.check(),
                    details: component.details(),
                });
            }
        }

        None
    }

    /// 등록된 컴포넌트 목록
    pub fn list_components(&self) -> Vec<String> {
        let components = self.components.read().unwrap_or_else(|e| e.into_inner());
        components.iter().map(|c| c.name().to_string()).collect()
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// 컴포넌트 상태
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStatus {
    pub status: HealthStatus,
    pub details: HashMap<String, String>,
}

/// 헬스 체크 결과
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub status: HealthStatus,
    pub components: HashMap<String, ComponentStatus>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl HealthCheckResult {
    /// JSON으로 변환
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

// ============================================================================
// LivenessProbe - 생존 확인
// ============================================================================

/// Liveness 프로브 (서버 생존 확인)
pub struct LivenessProbe {
    /// 마지막 활동 시간
    last_activity: RwLock<Instant>,
    /// 타임아웃
    timeout: Duration,
}

impl LivenessProbe {
    /// 새 Liveness 프로브 생성
    pub fn new(timeout: Duration) -> Self {
        Self {
            last_activity: RwLock::new(Instant::now()),
            timeout,
        }
    }

    /// 활동 기록 (heartbeat)
    pub fn heartbeat(&self) {
        let mut last = self
            .last_activity
            .write()
            .unwrap_or_else(|e| e.into_inner());
        *last = Instant::now();
    }

    /// 생존 확인
    pub fn is_alive(&self) -> bool {
        let last = self.last_activity.read().unwrap_or_else(|e| e.into_inner());
        last.elapsed() < self.timeout
    }

    /// 상태 체크
    pub fn check(&self) -> HealthStatus {
        if self.is_alive() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy(format!(
                "No activity for {:?}",
                self.last_activity
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .elapsed()
            ))
        }
    }
}

impl ComponentHealth for LivenessProbe {
    fn name(&self) -> &str {
        "liveness"
    }

    fn check(&self) -> HealthStatus {
        LivenessProbe::check(self)
    }

    fn details(&self) -> HashMap<String, String> {
        let mut details = HashMap::new();
        let last = self.last_activity.read().unwrap_or_else(|e| e.into_inner());
        details.insert(
            "last_activity_ago_ms".to_string(),
            last.elapsed().as_millis().to_string(),
        );
        details.insert(
            "timeout_ms".to_string(),
            self.timeout.as_millis().to_string(),
        );
        details
    }
}

// ============================================================================
// ReadinessProbe - 준비 확인
// ============================================================================

/// Readiness 프로브 (서비스 준비 확인)
pub struct ReadinessProbe {
    /// 의존성 체크 함수들
    dependencies: RwLock<Vec<Box<dyn Fn() -> HealthStatus + Send + Sync>>>,
    /// 준비 상태 플래그
    ready: RwLock<bool>,
}

impl ReadinessProbe {
    /// 새 Readiness 프로브 생성
    pub fn new() -> Self {
        Self {
            dependencies: RwLock::new(Vec::new()),
            ready: RwLock::new(false),
        }
    }

    /// 준비 완료 표시
    pub fn mark_ready(&self) {
        let mut ready = self.ready.write().unwrap_or_else(|e| e.into_inner());
        *ready = true;
    }

    /// 준비 안됨 표시
    pub fn mark_not_ready(&self) {
        let mut ready = self.ready.write().unwrap_or_else(|e| e.into_inner());
        *ready = false;
    }

    /// 의존성 추가
    pub fn add_dependency<F>(&self, check: F)
    where
        F: Fn() -> HealthStatus + Send + Sync + 'static,
    {
        let mut deps = self.dependencies.write().unwrap_or_else(|e| e.into_inner());
        deps.push(Box::new(check));
    }

    /// 준비 상태 확인
    pub fn is_ready(&self) -> bool {
        let ready = self.ready.read().unwrap_or_else(|e| e.into_inner());
        if !*ready {
            return false;
        }

        // 모든 의존성 체크
        let deps = self.dependencies.read().unwrap_or_else(|e| e.into_inner());
        for dep in deps.iter() {
            if !dep().is_available() {
                return false;
            }
        }

        true
    }

    /// 상태 체크
    pub fn check(&self) -> HealthStatus {
        let ready = self.ready.read().unwrap_or_else(|e| e.into_inner());
        if !*ready {
            return HealthStatus::Unhealthy("Service not ready".to_string());
        }

        // 의존성 체크
        let deps = self.dependencies.read().unwrap_or_else(|e| e.into_inner());
        let mut degraded_reasons = Vec::new();

        for dep in deps.iter() {
            match dep() {
                HealthStatus::Unhealthy(reason) => {
                    return HealthStatus::Unhealthy(format!("Dependency failed: {}", reason));
                }
                HealthStatus::Degraded(reason) => {
                    degraded_reasons.push(reason);
                }
                HealthStatus::Healthy => {}
            }
        }

        if !degraded_reasons.is_empty() {
            return HealthStatus::Degraded(degraded_reasons.join(", "));
        }

        HealthStatus::Healthy
    }
}

impl Default for ReadinessProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentHealth for ReadinessProbe {
    fn name(&self) -> &str {
        "readiness"
    }

    fn check(&self) -> HealthStatus {
        ReadinessProbe::check(self)
    }

    fn details(&self) -> HashMap<String, String> {
        let mut details = HashMap::new();
        let ready = self.ready.read().unwrap_or_else(|e| e.into_inner());
        details.insert("ready".to_string(), ready.to_string());
        let deps = self.dependencies.read().unwrap_or_else(|e| e.into_inner());
        details.insert("dependency_count".to_string(), deps.len().to_string());
        details
    }
}

// ============================================================================
// Built-in Health Checks
// ============================================================================

/// 데이터베이스 헬스 체크
pub struct DatabaseHealthCheck {
    name: String,
    check_fn: Box<dyn Fn() -> HealthStatus + Send + Sync>,
}

impl DatabaseHealthCheck {
    /// 새 데이터베이스 헬스 체크 생성
    pub fn new<F>(name: &str, check_fn: F) -> Self
    where
        F: Fn() -> HealthStatus + Send + Sync + 'static,
    {
        Self {
            name: format!("database:{}", name),
            check_fn: Box::new(check_fn),
        }
    }
}

impl ComponentHealth for DatabaseHealthCheck {
    fn name(&self) -> &str {
        &self.name
    }

    fn check(&self) -> HealthStatus {
        (self.check_fn)()
    }
}

/// 디스크 공간 헬스 체크
pub struct DiskSpaceHealthCheck {
    path: String,
    warning_threshold: f64,  // 경고 임계값 (사용률)
    critical_threshold: f64, // 심각 임계값 (사용률)
    // 시뮬레이션을 위한 필드
    simulated_usage: RwLock<Option<f64>>,
    // 캐시된 디스크 정보
    cached_info: RwLock<Option<DiskInfo>>,
    cache_time: RwLock<Option<Instant>>,
}

/// 디스크 정보
#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub usage_ratio: f64,
    pub mount_point: String,
    pub file_system: String,
}

impl DiskSpaceHealthCheck {
    /// 새 디스크 공간 헬스 체크 생성
    pub fn new(path: &str, warning_threshold: f64, critical_threshold: f64) -> Self {
        Self {
            path: path.to_string(),
            warning_threshold,
            critical_threshold,
            simulated_usage: RwLock::new(None),
            cached_info: RwLock::new(None),
            cache_time: RwLock::new(None),
        }
    }

    /// 기본 임계값으로 생성 (경고: 80%, 심각: 95%)
    pub fn with_defaults(path: &str) -> Self {
        Self::new(path, 0.80, 0.95)
    }

    /// 캐시 유효 시간 (10초)
    const CACHE_TTL: Duration = Duration::from_secs(10);

    fn get_disk_usage(&self) -> f64 {
        // 시뮬레이션된 값이 있으면 사용
        if let Some(usage) = *self
            .simulated_usage
            .read()
            .unwrap_or_else(|e| e.into_inner())
        {
            return usage;
        }

        // 캐시 확인
        {
            let cache_time = self.cache_time.read().unwrap_or_else(|e| e.into_inner());
            let cached_info = self.cached_info.read().unwrap_or_else(|e| e.into_inner());
            if let (Some(time), Some(info)) = (cache_time.as_ref(), cached_info.as_ref())
                && time.elapsed() < Self::CACHE_TTL
            {
                return info.usage_ratio;
            }
        }

        // 실제 디스크 사용량 확인
        let info = self.get_disk_info_internal();
        let usage = info.usage_ratio;

        // 캐시 업데이트
        {
            let mut cache_time = self.cache_time.write().unwrap_or_else(|e| e.into_inner());
            let mut cached_info = self.cached_info.write().unwrap_or_else(|e| e.into_inner());
            *cache_time = Some(Instant::now());
            *cached_info = Some(info);
        }

        usage
    }

    /// 상세 디스크 정보 조회
    pub fn get_disk_info(&self) -> DiskInfo {
        // 캐시 확인
        {
            let cache_time = self.cache_time.read().unwrap_or_else(|e| e.into_inner());
            let cached_info = self.cached_info.read().unwrap_or_else(|e| e.into_inner());
            if let (Some(time), Some(info)) = (cache_time.as_ref(), cached_info.as_ref())
                && time.elapsed() < Self::CACHE_TTL
            {
                return info.clone();
            }
        }

        let info = self.get_disk_info_internal();

        // 캐시 업데이트
        {
            let mut cache_time = self.cache_time.write().unwrap_or_else(|e| e.into_inner());
            let mut cached_info = self.cached_info.write().unwrap_or_else(|e| e.into_inner());
            *cache_time = Some(Instant::now());
            *cached_info = Some(info.clone());
        }

        info
    }

    fn get_disk_info_internal(&self) -> DiskInfo {
        let disks = Disks::new_with_refreshed_list();
        let target_path = Path::new(&self.path);

        // 경로에 해당하는 디스크 찾기
        let mut best_match: Option<(&sysinfo::Disk, usize)> = None;

        for disk in disks.list() {
            let mount_point = disk.mount_point();
            if target_path.starts_with(mount_point) {
                let mount_len = mount_point.as_os_str().len();
                if best_match.as_ref().is_none_or(|m| mount_len > m.1) {
                    best_match = Some((disk, mount_len));
                }
            }
        }

        if let Some((disk, _)) = best_match {
            let total = disk.total_space();
            let available = disk.available_space();
            let used = total.saturating_sub(available);
            let usage = if total > 0 {
                used as f64 / total as f64
            } else {
                0.0
            };

            DiskInfo {
                total_bytes: total,
                available_bytes: available,
                used_bytes: used,
                usage_ratio: usage,
                mount_point: disk.mount_point().to_string_lossy().to_string(),
                file_system: disk.file_system().to_string_lossy().to_string(),
            }
        } else {
            // 디스크를 찾지 못한 경우 기본값
            DiskInfo {
                total_bytes: 0,
                available_bytes: 0,
                used_bytes: 0,
                usage_ratio: 0.0,
                mount_point: "unknown".to_string(),
                file_system: "unknown".to_string(),
            }
        }
    }
}

impl ComponentHealth for DiskSpaceHealthCheck {
    fn name(&self) -> &str {
        "disk_space"
    }

    fn check(&self) -> HealthStatus {
        let usage = self.get_disk_usage();

        if usage >= self.critical_threshold {
            HealthStatus::Unhealthy(format!(
                "Disk space critical: {:.1}% used on {}",
                usage * 100.0,
                self.path
            ))
        } else if usage >= self.warning_threshold {
            HealthStatus::Degraded(format!(
                "Disk space warning: {:.1}% used on {}",
                usage * 100.0,
                self.path
            ))
        } else {
            HealthStatus::Healthy
        }
    }

    fn details(&self) -> HashMap<String, String> {
        let mut details = HashMap::new();
        let info = self.get_disk_info();
        details.insert("path".to_string(), self.path.clone());
        details.insert("mount_point".to_string(), info.mount_point);
        details.insert("file_system".to_string(), info.file_system);
        details.insert(
            "usage_percent".to_string(),
            format!("{:.1}", info.usage_ratio * 100.0),
        );
        details.insert(
            "total_gb".to_string(),
            format!("{:.2}", info.total_bytes as f64 / 1_073_741_824.0),
        );
        details.insert(
            "used_gb".to_string(),
            format!("{:.2}", info.used_bytes as f64 / 1_073_741_824.0),
        );
        details.insert(
            "available_gb".to_string(),
            format!("{:.2}", info.available_bytes as f64 / 1_073_741_824.0),
        );
        details
    }
}

/// 메모리 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_ratio: f64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

/// 메모리 헬스 체크
pub struct MemoryHealthCheck {
    warning_threshold: f64,
    critical_threshold: f64,
    // 시뮬레이션을 위한 필드
    simulated_usage: RwLock<Option<f64>>,
    // 캐시
    cached_info: RwLock<Option<MemoryInfo>>,
    cache_time: RwLock<Option<Instant>>,
}

impl MemoryHealthCheck {
    /// 새 메모리 헬스 체크 생성
    pub fn new(warning_threshold: f64, critical_threshold: f64) -> Self {
        Self {
            warning_threshold,
            critical_threshold,
            simulated_usage: RwLock::new(None),
            cached_info: RwLock::new(None),
            cache_time: RwLock::new(None),
        }
    }

    /// 기본 임계값으로 생성 (경고: 80%, 심각: 95%)
    pub fn with_defaults() -> Self {
        Self::new(0.80, 0.95)
    }

    /// 캐시 유효 시간 (5초)
    const CACHE_TTL: Duration = Duration::from_secs(5);

    fn get_memory_usage(&self) -> f64 {
        if let Some(usage) = *self
            .simulated_usage
            .read()
            .unwrap_or_else(|e| e.into_inner())
        {
            return usage;
        }

        self.get_memory_info().usage_ratio
    }

    /// 상세 메모리 정보 조회
    pub fn get_memory_info(&self) -> MemoryInfo {
        // 캐시 확인
        {
            let cache_time = self.cache_time.read().unwrap_or_else(|e| e.into_inner());
            let cached_info = self.cached_info.read().unwrap_or_else(|e| e.into_inner());
            if let (Some(time), Some(info)) = (cache_time.as_ref(), cached_info.as_ref())
                && time.elapsed() < Self::CACHE_TTL
            {
                return info.clone();
            }
        }

        let info = self.get_memory_info_internal();

        // 캐시 업데이트
        {
            let mut cache_time = self.cache_time.write().unwrap_or_else(|e| e.into_inner());
            let mut cached_info = self.cached_info.write().unwrap_or_else(|e| e.into_inner());
            *cache_time = Some(Instant::now());
            *cached_info = Some(info.clone());
        }

        info
    }

    fn get_memory_info_internal(&self) -> MemoryInfo {
        let mut sys = System::new();
        sys.refresh_memory();

        let total = sys.total_memory();
        let used = sys.used_memory();
        let available = total.saturating_sub(used);
        let usage = if total > 0 {
            used as f64 / total as f64
        } else {
            0.0
        };

        MemoryInfo {
            total_bytes: total,
            used_bytes: used,
            available_bytes: available,
            usage_ratio: usage,
            swap_total_bytes: sys.total_swap(),
            swap_used_bytes: sys.used_swap(),
        }
    }
}

impl ComponentHealth for MemoryHealthCheck {
    fn name(&self) -> &str {
        "memory"
    }

    fn check(&self) -> HealthStatus {
        let usage = self.get_memory_usage();

        if usage >= self.critical_threshold {
            HealthStatus::Unhealthy(format!("Memory critical: {:.1}% used", usage * 100.0))
        } else if usage >= self.warning_threshold {
            HealthStatus::Degraded(format!("Memory warning: {:.1}% used", usage * 100.0))
        } else {
            HealthStatus::Healthy
        }
    }

    fn details(&self) -> HashMap<String, String> {
        let mut details = HashMap::new();
        let info = self.get_memory_info();
        details.insert(
            "usage_percent".to_string(),
            format!("{:.1}", info.usage_ratio * 100.0),
        );
        details.insert(
            "total_gb".to_string(),
            format!("{:.2}", info.total_bytes as f64 / 1_073_741_824.0),
        );
        details.insert(
            "used_gb".to_string(),
            format!("{:.2}", info.used_bytes as f64 / 1_073_741_824.0),
        );
        details.insert(
            "available_gb".to_string(),
            format!("{:.2}", info.available_bytes as f64 / 1_073_741_824.0),
        );
        details.insert(
            "swap_total_gb".to_string(),
            format!("{:.2}", info.swap_total_bytes as f64 / 1_073_741_824.0),
        );
        details.insert(
            "swap_used_gb".to_string(),
            format!("{:.2}", info.swap_used_bytes as f64 / 1_073_741_824.0),
        );
        details
    }
}

// ============================================================================
// CPU Health Check (v1.5.0 추가)
// ============================================================================

/// CPU 정보
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CpuInfo {
    pub usage_ratio: f64,
    pub core_count: usize,
    pub load_average: (f64, f64, f64), // 1분, 5분, 15분
}

/// CPU 헬스 체크
#[allow(dead_code)]
pub struct CpuHealthCheck {
    warning_threshold: f64,
    critical_threshold: f64,
    simulated_usage: RwLock<Option<f64>>,
    cached_info: RwLock<Option<CpuInfo>>,
    cache_time: RwLock<Option<Instant>>,
}

#[allow(dead_code)]
impl CpuHealthCheck {
    /// 새 CPU 헬스 체크 생성
    pub fn new(warning_threshold: f64, critical_threshold: f64) -> Self {
        Self {
            warning_threshold,
            critical_threshold,
            simulated_usage: RwLock::new(None),
            cached_info: RwLock::new(None),
            cache_time: RwLock::new(None),
        }
    }

    /// 기본 임계값으로 생성 (경고: 80%, 심각: 95%)
    pub fn with_defaults() -> Self {
        Self::new(0.80, 0.95)
    }

    const CACHE_TTL: Duration = Duration::from_secs(5);

    fn get_cpu_usage(&self) -> f64 {
        if let Some(usage) = *self
            .simulated_usage
            .read()
            .unwrap_or_else(|e| e.into_inner())
        {
            return usage;
        }
        self.get_cpu_info().usage_ratio
    }

    /// 상세 CPU 정보 조회
    pub fn get_cpu_info(&self) -> CpuInfo {
        // 캐시 확인
        {
            let cache_time = self.cache_time.read().unwrap_or_else(|e| e.into_inner());
            let cached_info = self.cached_info.read().unwrap_or_else(|e| e.into_inner());
            if let (Some(time), Some(info)) = (cache_time.as_ref(), cached_info.as_ref())
                && time.elapsed() < Self::CACHE_TTL
            {
                return info.clone();
            }
        }

        let info = self.get_cpu_info_internal();

        // 캐시 업데이트
        {
            let mut cache_time = self.cache_time.write().unwrap_or_else(|e| e.into_inner());
            let mut cached_info = self.cached_info.write().unwrap_or_else(|e| e.into_inner());
            *cache_time = Some(Instant::now());
            *cached_info = Some(info.clone());
        }

        info
    }

    fn get_cpu_info_internal(&self) -> CpuInfo {
        let mut sys = System::new();
        sys.refresh_cpu_all();

        // CPU 사용률은 두 번 측정 필요 (간격 필요)
        std::thread::sleep(Duration::from_millis(100));
        sys.refresh_cpu_all();

        let cpu_usage = sys.global_cpu_usage() as f64 / 100.0;
        let core_count = sys.cpus().len();

        // Load average (Unix only)
        let load_avg = System::load_average();

        CpuInfo {
            usage_ratio: cpu_usage,
            core_count,
            load_average: (load_avg.one, load_avg.five, load_avg.fifteen),
        }
    }
}

impl ComponentHealth for CpuHealthCheck {
    fn name(&self) -> &str {
        "cpu"
    }

    fn check(&self) -> HealthStatus {
        let usage = self.get_cpu_usage();

        if usage >= self.critical_threshold {
            HealthStatus::Unhealthy(format!("CPU critical: {:.1}% used", usage * 100.0))
        } else if usage >= self.warning_threshold {
            HealthStatus::Degraded(format!("CPU warning: {:.1}% used", usage * 100.0))
        } else {
            HealthStatus::Healthy
        }
    }

    fn details(&self) -> HashMap<String, String> {
        let mut details = HashMap::new();
        let info = self.get_cpu_info();
        details.insert(
            "usage_percent".to_string(),
            format!("{:.1}", info.usage_ratio * 100.0),
        );
        details.insert("core_count".to_string(), info.core_count.to_string());
        details.insert(
            "load_avg_1m".to_string(),
            format!("{:.2}", info.load_average.0),
        );
        details.insert(
            "load_avg_5m".to_string(),
            format!("{:.2}", info.load_average.1),
        );
        details.insert(
            "load_avg_15m".to_string(),
            format!("{:.2}", info.load_average.2),
        );
        details
    }
}

// ============================================================================
// Connection Pool Health Check (v1.5.0 추가)
// ============================================================================

/// 연결 풀 헬스 체크
#[allow(dead_code)]
pub struct ConnectionPoolHealthCheck {
    name: String,
    get_stats: Box<dyn Fn() -> ConnectionPoolStats + Send + Sync>,
    warning_threshold: f64,  // 사용률 경고 임계값
    critical_threshold: f64, // 사용률 심각 임계값
}

/// 연결 풀 통계
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct ConnectionPoolStats {
    pub total_connections: usize,
    pub active_connections: usize,
    pub idle_connections: usize,
    pub waiting_requests: usize,
    pub max_connections: usize,
}

#[allow(dead_code)]
impl ConnectionPoolStats {
    /// 사용률 계산
    pub fn usage_ratio(&self) -> f64 {
        if self.max_connections == 0 {
            0.0
        } else {
            self.active_connections as f64 / self.max_connections as f64
        }
    }
}

#[allow(dead_code)]
impl ConnectionPoolHealthCheck {
    /// 새 연결 풀 헬스 체크 생성
    pub fn new<F>(name: &str, get_stats: F, warning_threshold: f64, critical_threshold: f64) -> Self
    where
        F: Fn() -> ConnectionPoolStats + Send + Sync + 'static,
    {
        Self {
            name: format!("connection_pool:{}", name),
            get_stats: Box::new(get_stats),
            warning_threshold,
            critical_threshold,
        }
    }

    /// 기본 임계값으로 생성 (경고: 80%, 심각: 95%)
    pub fn with_defaults<F>(name: &str, get_stats: F) -> Self
    where
        F: Fn() -> ConnectionPoolStats + Send + Sync + 'static,
    {
        Self::new(name, get_stats, 0.80, 0.95)
    }
}

impl ComponentHealth for ConnectionPoolHealthCheck {
    fn name(&self) -> &str {
        &self.name
    }

    fn check(&self) -> HealthStatus {
        let stats = (self.get_stats)();
        let usage = stats.usage_ratio();

        if usage >= self.critical_threshold {
            HealthStatus::Unhealthy(format!(
                "Connection pool critical: {}/{} ({:.1}%)",
                stats.active_connections,
                stats.max_connections,
                usage * 100.0
            ))
        } else if usage >= self.warning_threshold {
            HealthStatus::Degraded(format!(
                "Connection pool warning: {}/{} ({:.1}%)",
                stats.active_connections,
                stats.max_connections,
                usage * 100.0
            ))
        } else {
            HealthStatus::Healthy
        }
    }

    fn details(&self) -> HashMap<String, String> {
        let stats = (self.get_stats)();
        let mut details = HashMap::new();
        details.insert("total".to_string(), stats.total_connections.to_string());
        details.insert("active".to_string(), stats.active_connections.to_string());
        details.insert("idle".to_string(), stats.idle_connections.to_string());
        details.insert("waiting".to_string(), stats.waiting_requests.to_string());
        details.insert("max".to_string(), stats.max_connections.to_string());
        details.insert(
            "usage_percent".to_string(),
            format!("{:.1}", stats.usage_ratio() * 100.0),
        );
        details
    }
}

// ============================================================================
// System Health Summary (v1.5.0 추가)
// ============================================================================

/// 시스템 상태 요약
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealthSummary {
    pub uptime_seconds: u64,
    pub memory: MemoryInfo,
    pub cpu_usage: f64,
    pub disk_usage: f64,
    pub process_count: usize,
}

#[allow(dead_code)]
impl SystemHealthSummary {
    /// 시스템 상태 요약 수집
    pub fn collect() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();

        let memory = MemoryInfo {
            total_bytes: sys.total_memory(),
            used_bytes: sys.used_memory(),
            available_bytes: sys.total_memory().saturating_sub(sys.used_memory()),
            usage_ratio: if sys.total_memory() > 0 {
                sys.used_memory() as f64 / sys.total_memory() as f64
            } else {
                0.0
            },
            swap_total_bytes: sys.total_swap(),
            swap_used_bytes: sys.used_swap(),
        };

        Self {
            uptime_seconds: System::uptime(),
            memory,
            cpu_usage: sys.global_cpu_usage() as f64 / 100.0,
            disk_usage: 0.0, // 별도 조회 필요
            process_count: sys.processes().len(),
        }
    }
}
