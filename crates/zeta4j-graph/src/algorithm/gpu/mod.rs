//! GPU 가속 모듈
//!
//! 그래프 알고리즘의 GPU 병렬 처리를 지원합니다.
//!
//! # 지원 백엔드
//!
//! - **CUDA**: NVIDIA GPU (Linux/Windows)
//! - **Metal**: Apple Silicon (macOS)
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::algorithm::gpu::{auto_select_backend, is_gpu_available};
//!
//! // GPU 사용 가능 여부 확인
//! if is_gpu_available() {
//!     let backend = auto_select_backend().unwrap();
//!     println!("Using GPU backend: {}", backend.name());
//! }
//! ```
//!
//! # Feature Flags
//!
//! - `gpu-cuda`: CUDA 백엔드 활성화
//! - `gpu-metal`: Metal 백엔드 활성화 (macOS 전용)

pub mod backend;
pub mod bfs;
pub mod community;
pub mod error;
pub mod graph_repr;
pub mod node2vec;
pub mod pagerank;

#[cfg(feature = "gpu-cuda")]
pub mod cuda;

#[cfg(all(feature = "gpu-metal", target_os = "macos"))]
pub mod metal;

// Re-exports
pub use backend::{BackendType, GpuBackend, GpuBuffer, GpuDeviceInfo, KernelArg};
pub use bfs::{BfsGpu, BfsGpuConfig, BfsGpuResult, SsspGpu, SsspGpuConfig, SsspGpuResult};
pub use community::{
    LabelPropagationGpu, LabelPropagationGpuConfig, LabelPropagationGpuResult, TriangleCountingGpu,
    TriangleCountingGpuConfig, TriangleCountingGpuResult,
};
pub use error::{GpuError, GpuResult};
pub use graph_repr::{BidirectionalCsrGraph, CsrGraph, GpuCsrGraph};
pub use node2vec::{Node2VecGpu, Node2VecGpuConfig, Node2VecGpuResult};
pub use pagerank::{PageRankGpu, PageRankGpuConfig, PageRankGpuResult};

#[cfg(feature = "gpu-cuda")]
pub use cuda::CudaBackend;

#[cfg(all(feature = "gpu-metal", target_os = "macos"))]
pub use metal::MetalBackend;

/// GPU 사용 가능 여부 확인
pub fn is_gpu_available() -> bool {
    #[cfg(feature = "gpu-cuda")]
    {
        if cuda::CudaBackend::is_available() {
            return true;
        }
    }

    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    {
        if metal::MetalBackend::is_available() {
            return true;
        }
    }

    false
}

/// 사용 가능한 최적 백엔드 자동 선택
///
/// 우선순위:
/// 1. CUDA (NVIDIA GPU)
/// 2. Metal (Apple Silicon)
pub fn auto_select_backend() -> GpuResult<Box<dyn GpuBackend>> {
    #[cfg(feature = "gpu-cuda")]
    {
        if cuda::CudaBackend::is_available() {
            return Ok(Box::new(cuda::CudaBackend::new()?));
        }
    }

    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    {
        if metal::MetalBackend::is_available() {
            return Ok(Box::new(metal::MetalBackend::new()?));
        }
    }

    Err(GpuError::NotAvailable)
}

/// 사용 가능한 모든 백엔드 목록
pub fn available_backends() -> Vec<BackendType> {
    let mut backends = Vec::new();

    #[cfg(feature = "gpu-cuda")]
    {
        if cuda::CudaBackend::is_available() {
            backends.push(BackendType::Cuda);
        }
    }

    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    {
        if metal::MetalBackend::is_available() {
            backends.push(BackendType::Metal);
        }
    }

    backends
}
