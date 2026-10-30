//! Metal 백엔드
//!
//! Apple Silicon (M1/M2/M3/M4)을 위한 Metal 백엔드 구현입니다.
//!
//! # 요구사항
//!
//! - macOS 10.15+
//! - Apple Silicon 또는 AMD GPU
//! - Xcode Command Line Tools
//!
//! # Feature Flag
//!
//! `gpu-metal` 피처를 활성화해야 사용 가능합니다.
//!
//! ```toml
//! [dependencies]
//! zeta4j = { version = "1.84", features = ["gpu-metal"] }
//! ```

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use super::backend::{BackendType, GpuBackend, GpuBuffer, GpuDeviceInfo, KernelArg};
use super::error::{GpuError, GpuResult};

#[cfg(all(feature = "gpu-metal", target_os = "macos"))]
use metal::{CommandQueue, ComputePipelineState, Device, Library, MTLResourceOptions, MTLSize};

/// Metal 버퍼 ID (핸들로 사용)
#[derive(Debug, Clone, Copy)]
struct MetalBufferId(u64);

/// Metal 버퍼 핸들 (실제 Metal 버퍼 참조)
#[cfg(all(feature = "gpu-metal", target_os = "macos"))]
struct MetalBufferHandle {
    buffer: metal::Buffer,
    size: usize,
}

/// Metal 디바이스 컨텍스트
#[cfg(all(feature = "gpu-metal", target_os = "macos"))]
struct MetalContext {
    /// Metal 디바이스
    device: Device,
    /// 커맨드 큐
    command_queue: CommandQueue,
    /// 컴파일된 라이브러리 (셰이더)
    libraries: RwLock<HashMap<String, Library>>,
    /// 컴파일된 파이프라인 캐시
    pipelines: RwLock<HashMap<String, ComputePipelineState>>,
    /// 할당된 버퍼 목록 (핸들 ID -> MetalBufferHandle)
    buffers: RwLock<HashMap<u64, MetalBufferHandle>>,
    /// 다음 버퍼 ID
    next_buffer_id: RwLock<u64>,
    /// 디바이스 이름
    device_name: String,
    /// GPU Family
    gpu_family: String,
}

#[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
struct MetalContext {
    device_name: String,
    total_memory: usize,
    allocated_buffers: RwLock<Vec<usize>>,
    gpu_family: String,
}

#[cfg(all(feature = "gpu-metal", target_os = "macos"))]
impl MetalContext {
    /// 새 Metal 컨텍스트 생성
    fn new() -> GpuResult<Self> {
        let device = Device::system_default().ok_or(GpuError::NotAvailable)?;

        let device_name = device.name().to_string();
        let command_queue = device.new_command_queue();

        // GPU Family 결정
        let gpu_family = if device_name.contains("M1") {
            "Apple7"
        } else if device_name.contains("M2") {
            "Apple8"
        } else if device_name.contains("M3") || device_name.contains("M4") {
            "Apple9"
        } else {
            "Apple7"
        }
        .to_string();

        Ok(Self {
            device,
            command_queue,
            libraries: RwLock::new(HashMap::new()),
            pipelines: RwLock::new(HashMap::new()),
            buffers: RwLock::new(HashMap::new()),
            next_buffer_id: RwLock::new(1),
            device_name,
            gpu_family,
        })
    }

    /// 셰이더 소스 컴파일
    fn compile_shader(&self, name: &str, source: &str) -> GpuResult<()> {
        let options = metal::CompileOptions::new();

        let library = self
            .device
            .new_library_with_source(source, &options)
            .map_err(|e| GpuError::KernelCompileFailed {
                kernel_name: name.to_string(),
                error: e.to_string(),
            })?;

        self.libraries.write().insert(name.to_string(), library);
        Ok(())
    }

    /// 커널 함수에 대한 파이프라인 가져오기 (캐시됨)
    fn get_pipeline(
        &self,
        library_name: &str,
        kernel_name: &str,
    ) -> GpuResult<ComputePipelineState> {
        let cache_key = format!("{}::{}", library_name, kernel_name);

        // 캐시 확인
        if let Some(pipeline) = self.pipelines.read().get(&cache_key) {
            return Ok(pipeline.clone());
        }

        // 라이브러리에서 함수 가져오기
        let libraries = self.libraries.read();
        let library = libraries
            .get(library_name)
            .ok_or_else(|| GpuError::KernelNotFound {
                kernel_name: library_name.to_string(),
            })?;

        let function =
            library
                .get_function(kernel_name, None)
                .map_err(|_| GpuError::KernelNotFound {
                    kernel_name: kernel_name.to_string(),
                })?;

        // 파이프라인 생성
        let pipeline = self
            .device
            .new_compute_pipeline_state_with_function(&function)
            .map_err(|e| GpuError::KernelCompileFailed {
                kernel_name: kernel_name.to_string(),
                error: e.to_string(),
            })?;

        // 캐시에 저장
        drop(libraries); // 읽기 락 해제
        self.pipelines.write().insert(cache_key, pipeline.clone());

        Ok(pipeline)
    }

    /// 버퍼 할당
    fn allocate_buffer(&self, size: usize) -> GpuResult<u64> {
        let buffer = self
            .device
            .new_buffer(size as u64, MTLResourceOptions::StorageModeShared);

        let mut id = self.next_buffer_id.write();
        let buffer_id = *id;
        *id += 1;

        self.buffers
            .write()
            .insert(buffer_id, MetalBufferHandle { buffer, size });

        Ok(buffer_id)
    }

    /// 버퍼에 데이터 업로드
    fn upload_to_buffer(&self, data: &[u8]) -> GpuResult<u64> {
        let size = data.len();
        let buffer = self.device.new_buffer_with_data(
            data.as_ptr() as *const _,
            size as u64,
            MTLResourceOptions::StorageModeShared,
        );

        let mut id = self.next_buffer_id.write();
        let buffer_id = *id;
        *id += 1;

        self.buffers
            .write()
            .insert(buffer_id, MetalBufferHandle { buffer, size });

        Ok(buffer_id)
    }

    /// 버퍼에서 데이터 다운로드
    fn download_from_buffer(&self, buffer_id: u64, size: usize) -> GpuResult<Vec<u8>> {
        let buffers = self.buffers.read();
        let handle = buffers.get(&buffer_id).ok_or(GpuError::InvalidBuffer)?;
        if size > handle.size {
            return Err(GpuError::BufferSizeMismatch {
                expected: handle.size,
                actual: size,
            });
        }
        if size == 0 {
            return Ok(Vec::new());
        }

        let ptr = handle.buffer.contents() as *const u8;
        if ptr.is_null() {
            return Err(GpuError::MemoryCopyFailed(
                "Metal returned a null shared-buffer pointer".to_string(),
            ));
        }
        // SAFETY: Shared Metal buffers expose `handle.size` readable bytes for
        // their lifetime. Bounds and nullness are checked above, and the copy
        // completes while the read guard keeps the handle alive.
        let data = unsafe { std::slice::from_raw_parts(ptr, size).to_vec() };

        Ok(data)
    }

    /// Metal 버퍼 가져오기
    fn get_buffer(&self, buffer_id: u64) -> GpuResult<metal::Buffer> {
        let buffers = self.buffers.read();
        let handle = buffers.get(&buffer_id).ok_or(GpuError::InvalidBuffer)?;
        Ok(handle.buffer.clone())
    }

    /// 버퍼 해제
    fn free_buffer(&self, buffer_id: u64) -> GpuResult<()> {
        self.buffers.write().remove(&buffer_id);
        Ok(())
    }

    /// 사용 가능한 메모리
    fn available_memory(&self) -> usize {
        // Apple Silicon은 통합 메모리 사용
        let recommended = self.device.recommended_max_working_set_size() as usize;
        let current = self.device.current_allocated_size() as usize;
        recommended.saturating_sub(current)
    }

    /// 총 메모리
    fn total_memory(&self) -> usize {
        self.device.recommended_max_working_set_size() as usize
    }
}

#[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
impl MetalContext {
    fn new() -> GpuResult<Self> {
        #[cfg(target_os = "macos")]
        {
            let chip_name = std::process::Command::new("sysctl")
                .args(["-n", "machdep.cpu.brand_string"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_else(|| "Apple GPU".to_string())
                .trim()
                .to_string();

            let family = if chip_name.contains("M1") {
                "Apple7"
            } else if chip_name.contains("M2") {
                "Apple8"
            } else if chip_name.contains("M3") || chip_name.contains("M4") {
                "Apple9"
            } else {
                "Apple7"
            };

            Ok(Self {
                device_name: chip_name,
                total_memory: 16 * 1024 * 1024 * 1024,
                allocated_buffers: RwLock::new(Vec::new()),
                gpu_family: family.to_string(),
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(GpuError::NotAvailable)
        }
    }

    fn available_memory(&self) -> usize {
        let allocated: usize = self.allocated_buffers.read().iter().sum();
        self.total_memory.saturating_sub(allocated)
    }
}

/// Metal 백엔드
pub struct MetalBackend {
    /// Metal 컨텍스트
    context: Arc<MetalContext>,
}

/// GpuBuffer 핸들에서 버퍼 ID 추출
fn extract_buffer_id(buffer: &GpuBuffer) -> GpuResult<u64> {
    buffer
        .handle
        .downcast_ref::<MetalBufferId>()
        .map(|id| id.0)
        .ok_or(GpuError::InvalidBuffer)
}

impl MetalBackend {
    /// 새 Metal 백엔드 생성
    pub fn new() -> GpuResult<Self> {
        #[cfg(not(target_os = "macos"))]
        {
            return Err(GpuError::NotAvailable);
        }

        #[cfg(target_os = "macos")]
        {
            let context = MetalContext::new()?;
            let backend = Self {
                context: Arc::new(context),
            };

            // 기본 셰이더 컴파일
            #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
            {
                backend.compile_builtin_shaders()?;
            }

            Ok(backend)
        }
    }

    /// Metal 사용 가능 여부 확인 (정적 메서드)
    pub fn is_available() -> bool {
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        {
            Device::system_default().is_some()
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        {
            false
        }
    }

    /// 내장 셰이더 컴파일
    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    fn compile_builtin_shaders(&self) -> GpuResult<()> {
        // PageRank 셰이더
        let pagerank_source = include_str!("shaders/pagerank.metal");
        self.context.compile_shader("pagerank", pagerank_source)?;

        // BFS 셰이더
        let bfs_source = include_str!("shaders/bfs.metal");
        self.context.compile_shader("bfs", bfs_source)?;

        // Community 셰이더
        let community_source = include_str!("shaders/community.metal");
        self.context.compile_shader("community", community_source)?;

        // Node2Vec 셰이더
        let node2vec_source = include_str!("shaders/node2vec.metal");
        self.context.compile_shader("node2vec", node2vec_source)?;

        Ok(())
    }

    /// 커스텀 셰이더 컴파일
    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    pub fn compile_shader(&self, name: &str, source: &str) -> GpuResult<()> {
        self.context.compile_shader(name, source)
    }

    /// 커널 실행 (실제 Metal 구현)
    #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
    pub fn execute_kernel_real(
        &self,
        library_name: &str,
        kernel_name: &str,
        grid_size: [u32; 3],
        block_size: [u32; 3],
        buffers: &[u64],
        constants: &[&[u8]],
    ) -> GpuResult<()> {
        let pipeline = self.context.get_pipeline(library_name, kernel_name)?;

        let command_buffer = self.context.command_queue.new_command_buffer();
        let encoder = command_buffer.new_compute_command_encoder();

        encoder.set_compute_pipeline_state(&pipeline);

        // 버퍼 바인딩
        for (index, buffer_id) in buffers.iter().enumerate() {
            let buffer = self.context.get_buffer(*buffer_id)?;
            encoder.set_buffer(index as u64, Some(&buffer), 0);
        }

        // 상수 바인딩
        let buffer_count = buffers.len();
        for (index, constant_data) in constants.iter().enumerate() {
            encoder.set_bytes(
                (buffer_count + index) as u64,
                constant_data.len() as u64,
                constant_data.as_ptr() as *const _,
            );
        }

        // 디스패치
        let threads_per_grid = MTLSize::new(
            grid_size[0] as u64,
            grid_size[1] as u64,
            grid_size[2] as u64,
        );
        let threads_per_threadgroup = MTLSize::new(
            block_size[0] as u64,
            block_size[1] as u64,
            block_size[2] as u64,
        );

        encoder.dispatch_threads(threads_per_grid, threads_per_threadgroup);
        encoder.end_encoding();

        command_buffer.commit();
        command_buffer.wait_until_completed();

        // 실행 상태 확인 (MTLCommandBufferStatus)
        let status = command_buffer.status();
        if status == metal::MTLCommandBufferStatus::Error {
            return Err(GpuError::KernelExecutionFailed {
                kernel_name: kernel_name.to_string(),
                error: "Command buffer execution failed".to_string(),
            });
        }

        Ok(())
    }
}

impl GpuBackend for MetalBackend {
    fn name(&self) -> &'static str {
        "Metal"
    }

    fn backend_type(&self) -> BackendType {
        BackendType::Metal
    }

    fn check_available(&self) -> bool {
        Self::is_available()
    }

    fn device_info(&self) -> GpuDeviceInfo {
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        {
            GpuDeviceInfo {
                name: self.context.device_name.clone(),
                backend: BackendType::Metal,
                total_memory: self.context.total_memory(),
                available_memory: self.context.available_memory(),
                compute_capability: self.context.gpu_family.clone(),
                max_threads_per_block: 1024,
                max_grid_size: [2147483647, 65535, 65535],
                warp_size: 32,
            }
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        {
            GpuDeviceInfo {
                name: self.context.device_name.clone(),
                backend: BackendType::Metal,
                total_memory: 16 * 1024 * 1024 * 1024,
                available_memory: self.context.available_memory(),
                compute_capability: self.context.gpu_family.clone(),
                max_threads_per_block: 1024,
                max_grid_size: [2147483647, 65535, 65535],
                warp_size: 32,
            }
        }
    }

    fn allocate(&self, size: usize) -> GpuResult<GpuBuffer> {
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        {
            let buffer_id = self.context.allocate_buffer(size)?;
            Ok(GpuBuffer::new(
                MetalBufferId(buffer_id),
                size,
                BackendType::Metal,
            ))
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        {
            if size > self.context.available_memory() {
                return Err(GpuError::AllocationFailed {
                    requested_bytes: size,
                    available_bytes: Some(self.context.available_memory()),
                });
            }
            self.context.allocated_buffers.write().push(size);
            Ok(GpuBuffer::new(
                MetalBufferId(size as u64),
                size,
                BackendType::Metal,
            ))
        }
    }

    fn upload_bytes(&self, data: &[u8]) -> GpuResult<GpuBuffer> {
        let size = data.len();

        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        {
            let buffer_id = self.context.upload_to_buffer(data)?;
            Ok(GpuBuffer::new(
                MetalBufferId(buffer_id),
                size,
                BackendType::Metal,
            ))
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        {
            if size > self.context.available_memory() {
                return Err(GpuError::AllocationFailed {
                    requested_bytes: size,
                    available_bytes: Some(self.context.available_memory()),
                });
            }
            self.context.allocated_buffers.write().push(size);
            Ok(GpuBuffer::new(
                MetalBufferId(size as u64),
                size,
                BackendType::Metal,
            ))
        }
    }

    fn download_bytes(&self, buffer: &GpuBuffer) -> GpuResult<Vec<u8>> {
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        {
            let buffer_id = extract_buffer_id(buffer)?;
            self.context.download_from_buffer(buffer_id, buffer.size)
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        {
            Ok(vec![0u8; buffer.size])
        }
    }

    fn execute_kernel(
        &self,
        kernel_name: &str,
        grid_size: [u32; 3],
        block_size: [u32; 3],
        args: &[KernelArg],
    ) -> GpuResult<()> {
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        {
            // 라이브러리 이름과 커널 이름 분리 (format: "library::kernel")
            let (library_name, function_name) = if kernel_name.contains("::") {
                let parts: Vec<&str> = kernel_name.split("::").collect();
                (parts[0], parts[1])
            } else {
                ("pagerank", kernel_name) // 기본 라이브러리
            };

            // 버퍼와 상수 분리
            let mut buffer_ids = Vec::new();
            let mut constants: Vec<Vec<u8>> = Vec::new();

            for arg in args {
                match arg {
                    KernelArg::Buffer(buf) => {
                        let id = extract_buffer_id(buf)?;
                        buffer_ids.push(id);
                    }
                    KernelArg::Float(v) => {
                        constants.push(v.to_ne_bytes().to_vec());
                    }
                    KernelArg::Double(v) => {
                        constants.push(v.to_ne_bytes().to_vec());
                    }
                    KernelArg::Int32(v) => {
                        constants.push(v.to_ne_bytes().to_vec());
                    }
                    KernelArg::Int64(v) => {
                        constants.push(v.to_ne_bytes().to_vec());
                    }
                    KernelArg::UInt32(v) => {
                        constants.push(v.to_ne_bytes().to_vec());
                    }
                    KernelArg::UInt64(v) => {
                        constants.push(v.to_ne_bytes().to_vec());
                    }
                }
            }

            let constant_refs: Vec<&[u8]> = constants.iter().map(|v| v.as_slice()).collect();

            self.execute_kernel_real(
                library_name,
                function_name,
                grid_size,
                block_size,
                &buffer_ids,
                &constant_refs,
            )
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        {
            let _ = (kernel_name, grid_size, block_size, args);
            Ok(())
        }
    }

    fn synchronize(&self) -> GpuResult<()> {
        // Metal 커맨드 버퍼는 wait_until_completed()에서 이미 동기화됨
        Ok(())
    }

    fn free(&self, buffer: GpuBuffer) -> GpuResult<()> {
        #[cfg(all(feature = "gpu-metal", target_os = "macos"))]
        {
            let buffer_id = extract_buffer_id(&buffer)?;
            self.context.free_buffer(buffer_id)
        }

        #[cfg(not(all(feature = "gpu-metal", target_os = "macos")))]
        {
            let mut buffers = self.context.allocated_buffers.write();
            if let Some(pos) = buffers.iter().position(|&s| s == buffer.size) {
                buffers.remove(pos);
            }
            Ok(())
        }
    }

    fn available_memory(&self) -> GpuResult<usize> {
        Ok(self.context.available_memory())
    }
}
