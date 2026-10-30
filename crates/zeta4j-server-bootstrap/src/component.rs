//! Bootstrap Component Framework
//!
//! 서버 초기화를 위한 컴포넌트 기반 프레임워크
//!
//! # 개요
//!
//! Bootstrap 컴포넌트는 서버 시작 시 순차적으로 실행되는 초기화 단위입니다.
//! 각 컴포넌트는 우선순위를 가지며, 낮은 우선순위가 먼저 실행됩니다.
//!
//! # 예제
//!
//! ```ignore
//! let orchestrator = BootstrapOrchestrator::new()
//!     .register(DirectoryComponent)
//!     .register(LoggingComponent)
//!     .register(DatabaseComponent);
//!
//! orchestrator.initialize(&config)?;
//! ```

use std::fmt;

use super::ValidatedStartupConfig;
use zeta4j_common::Edition;

/// Bootstrap 결과 타입
pub type BootstrapResult<T> = Result<T, BootstrapError>;

/// Bootstrap 에러
#[derive(Debug)]
pub struct BootstrapError {
    /// 실패한 컴포넌트 이름
    pub component: &'static str,
    /// 에러 메시지
    pub message: String,
    /// 원본 에러 (있는 경우)
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl BootstrapError {
    /// 새 BootstrapError 생성
    pub fn new(component: &'static str, message: impl Into<String>) -> Self {
        Self {
            component,
            message: message.into(),
            source: None,
        }
    }

    /// 원본 에러와 함께 생성
    pub fn with_source<E>(component: &'static str, message: impl Into<String>, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self {
            component,
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.component, self.message)?;
        if let Some(ref source) = self.source {
            write!(f, ": {}", source)?;
        }
        Ok(())
    }
}

impl std::error::Error for BootstrapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

/// Bootstrap 컴포넌트 우선순위
///
/// 낮은 숫자가 먼저 실행됨
pub mod priority {
    /// 디렉토리 생성 (가장 먼저)
    pub const DIRECTORY: u32 = 10;
    /// 로깅 초기화
    pub const LOGGING: u32 = 20;
    /// 설정 로드
    pub const CONFIG: u32 = 30;
    /// TLS 설정
    pub const TLS: u32 = 40;
    /// 데이터베이스 초기화
    pub const DATABASE: u32 = 50;
    /// 서버 컴포넌트
    pub const SERVER: u32 = 60;
    /// 런타임 시작 (가장 마지막)
    pub const RUNTIME: u32 = 100;
}

/// Bootstrap 컴포넌트 인터페이스
///
/// 서버 초기화 시 실행되는 개별 컴포넌트를 정의합니다.
pub trait BootstrapComponent: Send + Sync {
    /// 컴포넌트 이름
    fn name(&self) -> &'static str;

    /// 초기화 우선순위 (낮을수록 먼저)
    ///
    /// 기본값: 100
    fn priority(&self) -> u32 {
        100
    }

    /// 컴포넌트 초기화 실행
    fn initialize(&self, edition: Edition, config: &ValidatedStartupConfig) -> BootstrapResult<()>;

    /// 컴포넌트 종료 (선택)
    ///
    /// 서버 종료 시 역순으로 호출됨
    fn shutdown(&self) -> BootstrapResult<()> {
        Ok(())
    }
}

/// Bootstrap 조율자
///
/// 여러 컴포넌트의 초기화를 관리합니다.
pub struct BootstrapOrchestrator {
    components: Vec<Box<dyn BootstrapComponent>>,
}

impl Default for BootstrapOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl BootstrapOrchestrator {
    /// 새 오케스트레이터 생성
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    /// 컴포넌트 등록
    pub fn register<C: BootstrapComponent + 'static>(mut self, component: C) -> Self {
        self.components.push(Box::new(component));
        self
    }

    /// 모든 컴포넌트 초기화 (우선순위순)
    pub fn initialize(
        &self,
        edition: Edition,
        config: &ValidatedStartupConfig,
    ) -> BootstrapResult<()> {
        let mut sorted: Vec<_> = self.components.iter().collect();
        sorted.sort_by_key(|c| c.priority());

        for component in sorted {
            log::debug!("Initializing bootstrap component: {}", component.name());
            component.initialize(edition, config)?;
            log::info!("Initialized: {}", component.name());
        }

        Ok(())
    }

    /// 모든 컴포넌트 종료 (역순)
    pub fn shutdown(&self) -> BootstrapResult<()> {
        let mut sorted: Vec<_> = self.components.iter().collect();
        sorted.sort_by_key(|c| std::cmp::Reverse(c.priority()));

        for component in sorted {
            log::debug!("Shutting down bootstrap component: {}", component.name());
            if let Err(e) = component.shutdown() {
                log::warn!("Error shutting down {}: {}", component.name(), e);
            }
        }

        Ok(())
    }

    /// 등록된 컴포넌트 수
    pub fn len(&self) -> usize {
        self.components.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }
}

// ============================================================================
// Built-in Components
// ============================================================================

/// 디렉토리 초기화 컴포넌트
pub struct DirectoryComponent;

impl BootstrapComponent for DirectoryComponent {
    fn name(&self) -> &'static str {
        "directory"
    }

    fn priority(&self) -> u32 {
        priority::DIRECTORY
    }

    fn initialize(
        &self,
        _edition: Edition,
        config: &ValidatedStartupConfig,
    ) -> BootstrapResult<()> {
        use super::DirectoryInit;

        DirectoryInit::new(config).create_all().map_err(|e| {
            BootstrapError::with_source(self.name(), "Failed to create directories", e)
        })
    }
}

/// 로깅 초기화 컴포넌트
pub struct LoggingComponent {
    log_config_path: Option<std::path::PathBuf>,
}

impl LoggingComponent {
    /// 새 LoggingComponent 생성
    pub fn new(log_config_path: Option<std::path::PathBuf>) -> Self {
        Self { log_config_path }
    }
}

impl BootstrapComponent for LoggingComponent {
    fn name(&self) -> &'static str {
        "logging"
    }

    fn priority(&self) -> u32 {
        priority::LOGGING
    }

    fn initialize(&self, edition: Edition, config: &ValidatedStartupConfig) -> BootstrapResult<()> {
        use super::init_logging;

        init_logging(edition, config, self.log_config_path.as_ref()).map_err(|e| {
            BootstrapError::new(self.name(), format!("Failed to initialize logging: {}", e))
        })
    }
}

/// 포트 충돌 검사 컴포넌트
pub struct PortCheckComponent {
    http_port: u16,
    bolt_port: u16,
}

impl PortCheckComponent {
    /// 새 PortCheckComponent 생성
    pub fn new(http_port: u16, bolt_port: u16) -> Self {
        Self {
            http_port,
            bolt_port,
        }
    }
}

impl BootstrapComponent for PortCheckComponent {
    fn name(&self) -> &'static str {
        "port-check"
    }

    fn priority(&self) -> u32 {
        priority::CONFIG + 5 // CONFIG 직후
    }

    fn initialize(
        &self,
        _edition: Edition,
        _config: &ValidatedStartupConfig,
    ) -> BootstrapResult<()> {
        use super::check_port_available;

        // HTTP 포트 확인
        if !check_port_available(self.http_port) {
            return Err(BootstrapError::new(
                self.name(),
                format!("HTTP port {} is already in use", self.http_port),
            ));
        }

        // Bolt 포트 확인
        if !check_port_available(self.bolt_port) {
            return Err(BootstrapError::new(
                self.name(),
                format!("Bolt port {} is already in use", self.bolt_port),
            ));
        }

        Ok(())
    }
}
