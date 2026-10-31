// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! GPU 백엔드 추상화
//!
//! 다양한 GPU 백엔드(CUDA, Metal)를 위한 공통 인터페이스를 정의합니다.

use std::fmt;
use std::sync::Arc;

use super::error::GpuResult;

/// GPU 백엔드 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendType {
    /// NVIDIA CUDA
    Cuda,
    /// Apple Metal
    Metal,
}

impl fmt::Display for BackendType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendType::Cuda => write!(f, "CUDA"),
            BackendType::Metal => write!(f, "Metal"),
        }
    }
}

/// GPU 디바이스 정보
#[derive(Debug, Clone)]
pub struct GpuDeviceInfo {
    /// 디바이스 이름
    pub name: String,
    /// 백엔드 종류
    pub backend: BackendType,
    /// 총 메모리 (bytes)
    pub total_memory: usize,
    /// 사용 가능한 메모리 (bytes)
    pub available_memory: usize,
    /// Compute capability (CUDA) 또는 GPU Family (Metal)
    pub compute_capability: String,
    /// 최대 스레드 블록 크기
    pub max_threads_per_block: usize,
    /// 최대 그리드 크기
    pub max_grid_size: [usize; 3],
    /// Warp/Wavefront 크기
    pub warp_size: usize,
}

impl GpuDeviceInfo {
    /// 새 디바이스 정보 생성
    pub fn new(name: &str, backend: BackendType) -> Self {
        Self {
            name: name.to_string(),
            backend,
            total_memory: 0,
            available_memory: 0,
            compute_capability: String::new(),
            max_threads_per_block: 256,
            max_grid_size: [65535, 65535, 65535],
            warp_size: 32,
        }
    }

    /// 메모리 사용률 (0.0 ~ 1.0)
    pub fn memory_usage(&self) -> f64 {
        if self.total_memory == 0 {
            0.0
        } else {
            1.0 - (self.available_memory as f64 / self.total_memory as f64)
        }
    }
}

/// GPU 버퍼 핸들
#[derive(Clone)]
pub struct GpuBuffer {
    /// 내부 핸들 (백엔드별 구현)
    pub(crate) handle: Arc<dyn std::any::Any + Send + Sync>,
    /// 버퍼 크기 (bytes)
    pub size: usize,
    /// 백엔드 종류
    pub backend: BackendType,
}

impl GpuBuffer {
    /// 새 버퍼 생성
    pub fn new<T: std::any::Any + Send + Sync + 'static>(
        handle: T,
        size: usize,
        backend: BackendType,
    ) -> Self {
        Self {
            handle: Arc::new(handle),
            size,
            backend,
        }
    }

    /// 요소 수 계산 (타입 크기로 나눔)
    pub fn len<T>(&self) -> usize {
        self.size / std::mem::size_of::<T>()
    }

    /// 빈 버퍼인지 확인
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
}

impl fmt::Debug for GpuBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuBuffer")
            .field("size", &self.size)
            .field("backend", &self.backend)
            .finish()
    }
}

/// 커널 인자
#[derive(Clone)]
pub enum KernelArg {
    /// GPU 버퍼
    Buffer(GpuBuffer),
    /// 부동소수점 스칼라
    Float(f32),
    /// 배정밀도 부동소수점 스칼라
    Double(f64),
    /// 32비트 정수
    Int32(i32),
    /// 64비트 정수
    Int64(i64),
    /// 부호 없는 32비트 정수
    UInt32(u32),
    /// 부호 없는 64비트 정수
    UInt64(u64),
}

impl fmt::Debug for KernelArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelArg::Buffer(buf) => write!(f, "Buffer({} bytes)", buf.size),
            KernelArg::Float(v) => write!(f, "Float({})", v),
            KernelArg::Double(v) => write!(f, "Double({})", v),
            KernelArg::Int32(v) => write!(f, "Int32({})", v),
            KernelArg::Int64(v) => write!(f, "Int64({})", v),
            KernelArg::UInt32(v) => write!(f, "UInt32({})", v),
            KernelArg::UInt64(v) => write!(f, "UInt64({})", v),
        }
    }
}

/// GPU 백엔드 트레이트
///
/// 모든 GPU 백엔드(CUDA, Metal)가 구현해야 하는 공통 인터페이스입니다.
pub trait GpuBackend: Send + Sync {
    /// 백엔드 이름
    fn name(&self) -> &'static str;

    /// 백엔드 종류
    fn backend_type(&self) -> BackendType;

    /// 사용 가능 여부 (인스턴스 메서드)
    fn check_available(&self) -> bool;

    /// 디바이스 정보 조회
    fn device_info(&self) -> GpuDeviceInfo;

    /// GPU 메모리 할당
    ///
    /// # Arguments
    /// * `size` - 할당할 바이트 수
    ///
    /// # Returns
    /// GPU 버퍼 핸들
    fn allocate(&self, size: usize) -> GpuResult<GpuBuffer>;

    /// 호스트 메모리 → GPU 메모리 복사 (바이트 기반)
    ///
    /// # Arguments
    /// * `data` - 복사할 호스트 데이터 (바이트)
    ///
    /// # Returns
    /// 데이터가 복사된 GPU 버퍼
    fn upload_bytes(&self, data: &[u8]) -> GpuResult<GpuBuffer>;

    /// GPU 메모리 → 호스트 메모리 복사 (바이트 기반)
    ///
    /// # Arguments
    /// * `buffer` - GPU 버퍼
    ///
    /// # Returns
    /// 호스트 메모리로 복사된 데이터 (바이트)
    fn download_bytes(&self, buffer: &GpuBuffer) -> GpuResult<Vec<u8>>;

    /// 커널 실행
    ///
    /// # Arguments
    /// * `kernel_name` - 커널 이름
    /// * `grid_size` - 그리드 크기 (블록 수)
    /// * `block_size` - 블록 크기 (스레드 수)
    /// * `args` - 커널 인자
    fn execute_kernel(
        &self,
        kernel_name: &str,
        grid_size: [u32; 3],
        block_size: [u32; 3],
        args: &[KernelArg],
    ) -> GpuResult<()>;

    /// GPU 연산 동기화 (완료 대기)
    fn synchronize(&self) -> GpuResult<()>;

    /// GPU 메모리 해제
    ///
    /// 참고: GpuBuffer가 Drop될 때 자동으로 호출됨
    fn free(&self, buffer: GpuBuffer) -> GpuResult<()>;

    /// 사용 가능한 GPU 메모리 조회
    fn available_memory(&self) -> GpuResult<usize>;

    /// 최적의 블록 크기 계산
    ///
    /// # Arguments
    /// * `kernel_name` - 커널 이름
    /// * `num_elements` - 처리할 요소 수
    ///
    /// # Returns
    /// (grid_size, block_size)
    fn optimal_launch_config(
        &self,
        _kernel_name: &str,
        num_elements: usize,
    ) -> GpuResult<([u32; 3], [u32; 3])> {
        let info = self.device_info();
        let block_size = info.max_threads_per_block.min(256) as u32;
        let grid_size = (num_elements as u32).div_ceil(block_size);

        Ok(([grid_size, 1, 1], [block_size, 1, 1]))
    }

    /// 타입 안전한 업로드 (제네릭 헬퍼)
    ///
    /// # Arguments
    /// * `data` - 복사할 호스트 데이터
    fn upload<T: bytemuck::Pod + Send + Sync + 'static>(&self, data: &[T]) -> GpuResult<GpuBuffer>
    where
        Self: Sized,
    {
        self.upload_bytes(bytemuck::cast_slice(data))
    }

    /// 타입 안전한 다운로드 (제네릭 헬퍼)
    ///
    /// # Arguments
    /// * `buffer` - GPU 버퍼
    fn download<T: bytemuck::Pod + Send + Sync + 'static>(
        &self,
        buffer: &GpuBuffer,
    ) -> GpuResult<Vec<T>>
    where
        Self: Sized,
    {
        let bytes = self.download_bytes(buffer)?;
        let element_size = std::mem::size_of::<T>();
        if element_size == 0 || !bytes.len().is_multiple_of(element_size) {
            return Err(GpuError::InvalidArgument(format!(
                "{} GPU bytes cannot be decoded as {}-byte elements",
                bytes.len(),
                element_size
            )));
        }
        Ok(bytemuck::allocation::pod_collect_to_vec(&bytes))
    }
}
