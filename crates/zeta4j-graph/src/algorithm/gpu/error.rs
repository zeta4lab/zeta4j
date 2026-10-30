//! GPU 에러 타입

use std::fmt;

/// GPU 연산 결과
pub type GpuResult<T> = Result<T, GpuError>;

/// GPU 에러 타입
#[derive(Debug, Clone)]
pub enum GpuError {
    /// GPU를 사용할 수 없음
    NotAvailable,

    /// GPU 디바이스 초기화 실패
    DeviceInitFailed(String),

    /// 메모리 할당 실패
    AllocationFailed {
        requested_bytes: usize,
        available_bytes: Option<usize>,
    },

    /// 메모리 복사 실패
    MemoryCopyFailed(String),

    /// 커널 컴파일 실패
    KernelCompileFailed { kernel_name: String, error: String },

    /// 커널 실행 실패
    KernelExecutionFailed { kernel_name: String, error: String },

    /// 동기화 실패
    SynchronizationFailed(String),

    /// 잘못된 인자
    InvalidArgument(String),

    /// 버퍼 크기 불일치
    BufferSizeMismatch { expected: usize, actual: usize },

    /// 지원하지 않는 연산
    UnsupportedOperation(String),

    /// 그래프 변환 실패
    GraphConversionFailed(String),

    /// 백엔드 특정 에러
    BackendError { backend: String, error: String },

    /// 커널을 찾을 수 없음
    KernelNotFound { kernel_name: String },

    /// 잘못된 버퍼
    InvalidBuffer,
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuError::NotAvailable => {
                write!(f, "GPU not available")
            }
            GpuError::DeviceInitFailed(msg) => {
                write!(f, "GPU device initialization failed: {}", msg)
            }
            GpuError::AllocationFailed {
                requested_bytes,
                available_bytes,
            } => match available_bytes {
                Some(avail) => write!(
                    f,
                    "GPU memory allocation failed: requested {} bytes, available {} bytes",
                    requested_bytes, avail
                ),
                None => write!(
                    f,
                    "GPU memory allocation failed: requested {} bytes",
                    requested_bytes
                ),
            },
            GpuError::MemoryCopyFailed(msg) => {
                write!(f, "GPU memory copy failed: {}", msg)
            }
            GpuError::KernelCompileFailed { kernel_name, error } => {
                write!(f, "Kernel '{}' compilation failed: {}", kernel_name, error)
            }
            GpuError::KernelExecutionFailed { kernel_name, error } => {
                write!(f, "Kernel '{}' execution failed: {}", kernel_name, error)
            }
            GpuError::SynchronizationFailed(msg) => {
                write!(f, "GPU synchronization failed: {}", msg)
            }
            GpuError::InvalidArgument(msg) => {
                write!(f, "Invalid argument: {}", msg)
            }
            GpuError::BufferSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "Buffer size mismatch: expected {} bytes, actual {} bytes",
                    expected, actual
                )
            }
            GpuError::UnsupportedOperation(op) => {
                write!(f, "Unsupported operation: {}", op)
            }
            GpuError::GraphConversionFailed(msg) => {
                write!(f, "Graph conversion failed: {}", msg)
            }
            GpuError::BackendError { backend, error } => {
                write!(f, "{} backend error: {}", backend, error)
            }
            GpuError::KernelNotFound { kernel_name } => {
                write!(f, "Kernel '{}' not found", kernel_name)
            }
            GpuError::InvalidBuffer => {
                write!(f, "Invalid GPU buffer")
            }
        }
    }
}

impl std::error::Error for GpuError {}

impl From<std::io::Error> for GpuError {
    fn from(err: std::io::Error) -> Self {
        GpuError::BackendError {
            backend: "IO".to_string(),
            error: err.to_string(),
        }
    }
}

impl From<crate::error::GraphError> for GpuError {
    fn from(err: crate::error::GraphError) -> Self {
        GpuError::GraphConversionFailed(err.to_string())
    }
}
