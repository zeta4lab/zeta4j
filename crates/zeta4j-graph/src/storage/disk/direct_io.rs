#![allow(unsafe_code)]
//! Direct I/O 지원 모듈
//!
//! OS 페이지 캐시를 우회하여 직접 디스크 I/O를 수행합니다.
//! 데이터베이스는 자체 버퍼 풀을 관리하므로 OS 캐시를 우회하면
//! 메모리 사용 효율성과 I/O 성능이 향상됩니다.
//!
//! # 플랫폼 지원
//! - Linux: O_DIRECT 플래그 사용
//! - macOS: F_NOCACHE fcntl 사용
//! - Windows: FILE_FLAG_NO_BUFFERING (미구현, 폴백)
//!
//! # 사용 예시
//! ```ignore
//! use zeta4j_graph::storage::direct_io::{DirectIoFile, DirectIoConfig, AlignedBuffer};
//!
//! let config = DirectIoConfig::default();
//! let file = DirectIoFile::open("data.db", &config)?;
//!
//! let mut buffer = AlignedBuffer::new(4096, 4096)?;
//! file.read_at(0, buffer.as_mut_slice())?;
//! ```

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;

#[cfg(target_os = "macos")]
use std::os::unix::io::AsRawFd;

use super::page::PAGE_SIZE;

// ============================================================================
// 상수
// ============================================================================

/// 기본 정렬 크기 (섹터 크기)
pub const DEFAULT_ALIGNMENT: usize = 4096;

/// 최소 정렬 크기
pub const MIN_ALIGNMENT: usize = 512;

// ============================================================================
// Direct I/O 설정
// ============================================================================

/// Direct I/O 설정
#[derive(Debug, Clone)]
pub struct DirectIoConfig {
    /// Direct I/O 활성화 여부
    pub enabled: bool,

    /// 정렬 크기 (바이트, 2의 거듭제곱이어야 함)
    pub alignment: usize,

    /// 읽기 버퍼 크기
    pub read_buffer_size: usize,

    /// 쓰기 버퍼 크기
    pub write_buffer_size: usize,

    /// fsync 호출 빈도 (쓰기 N회마다 1회)
    pub fsync_interval: usize,
}

impl Default for DirectIoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            alignment: DEFAULT_ALIGNMENT,
            read_buffer_size: PAGE_SIZE,
            write_buffer_size: PAGE_SIZE,
            fsync_interval: 1, // 매 쓰기마다 fsync
        }
    }
}

impl DirectIoConfig {
    /// Direct I/O 비활성화 설정
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// 고성능 설정 (fsync 간격 증가)
    pub fn high_performance() -> Self {
        Self {
            enabled: true,
            alignment: DEFAULT_ALIGNMENT,
            read_buffer_size: PAGE_SIZE * 4,
            write_buffer_size: PAGE_SIZE * 4,
            fsync_interval: 16,
        }
    }

    /// 정렬 크기 유효성 검사
    pub fn validate(&self) -> Result<(), DirectIoError> {
        if self.alignment < MIN_ALIGNMENT {
            return Err(DirectIoError::InvalidAlignment {
                alignment: self.alignment,
                min: MIN_ALIGNMENT,
            });
        }

        if !self.alignment.is_power_of_two() {
            return Err(DirectIoError::AlignmentNotPowerOfTwo {
                alignment: self.alignment,
            });
        }

        Ok(())
    }
}

// ============================================================================
// 정렬된 버퍼
// ============================================================================

/// 정렬된 메모리 버퍼
///
/// Direct I/O는 버퍼가 특정 경계에 정렬되어야 합니다.
/// 이 구조체는 정렬된 메모리를 할당하고 관리합니다.
#[derive(Debug)]
pub struct AlignedBuffer {
    /// 원본 할당 (정렬 패딩 포함)
    raw: Vec<u8>,

    /// 정렬된 시작 위치
    offset: usize,

    /// 사용 가능한 크기
    len: usize,

    /// 정렬 크기
    alignment: usize,
}

impl AlignedBuffer {
    /// 새로운 정렬된 버퍼 생성
    ///
    /// # Arguments
    /// * `size` - 버퍼 크기 (바이트)
    /// * `alignment` - 정렬 크기 (2의 거듭제곱)
    pub fn new(size: usize, alignment: usize) -> Result<Self, DirectIoError> {
        if !alignment.is_power_of_two() {
            return Err(DirectIoError::AlignmentNotPowerOfTwo { alignment });
        }
        if size == 0 {
            return Err(DirectIoError::InvalidBufferSize { size });
        }

        // 정렬을 위해 추가 공간 할당
        let total_size = size
            .checked_add(alignment)
            .ok_or(DirectIoError::AllocationSizeOverflow { size, alignment })?;
        let mut raw = Vec::new();
        raw.try_reserve_exact(total_size)
            .map_err(|error| DirectIoError::AllocationFailed {
                message: error.to_string(),
            })?;
        raw.resize(total_size, 0);

        // 정렬된 시작 주소 계산
        let raw_ptr = raw.as_mut_ptr();
        let offset = raw_ptr.align_offset(alignment);
        if offset == usize::MAX {
            return Err(DirectIoError::UnsatisfiableAlignment { alignment });
        }
        Ok(Self {
            raw,
            offset,
            len: size,
            alignment,
        })
    }

    /// PAGE_SIZE 크기의 버퍼 생성
    pub fn page_sized() -> Result<Self, DirectIoError> {
        Self::new(PAGE_SIZE, DEFAULT_ALIGNMENT)
    }

    /// 버퍼를 슬라이스로 반환
    pub fn as_slice(&self) -> &[u8] {
        &self.raw[self.offset..self.offset + self.len]
    }

    /// 버퍼를 가변 슬라이스로 반환
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.raw[self.offset..self.offset + self.len]
    }

    /// 버퍼 크기
    pub fn len(&self) -> usize {
        self.len
    }

    /// 버퍼가 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 정렬 크기
    pub fn alignment(&self) -> usize {
        self.alignment
    }

    /// 버퍼 초기화
    pub fn clear(&mut self) {
        self.as_mut_slice().fill(0);
    }

    /// 데이터 복사
    pub fn copy_from(&mut self, data: &[u8]) {
        let copy_len = std::cmp::min(data.len(), self.len);
        self.as_mut_slice()[..copy_len].copy_from_slice(&data[..copy_len]);
    }

    /// 정렬 확인
    pub fn is_aligned(&self) -> bool {
        (self.raw.as_ptr().wrapping_add(self.offset) as usize).is_multiple_of(self.alignment)
    }
}

// ============================================================================
// Direct I/O 파일
// ============================================================================

/// Direct I/O 지원 파일
#[derive(Debug)]
pub struct DirectIoFile {
    file: File,
    config: DirectIoConfig,
    write_count: usize,
    direct_io_enabled: bool,
}

impl DirectIoFile {
    /// 파일 열기 (없으면 생성)
    pub fn open(path: impl AsRef<Path>, config: &DirectIoConfig) -> Result<Self, DirectIoError> {
        config.validate()?;

        let (file, direct_io_enabled) = Self::open_with_direct_io(path.as_ref(), config)?;

        Ok(Self {
            file,
            config: config.clone(),
            write_count: 0,
            direct_io_enabled,
        })
    }

    /// 읽기 전용으로 열기
    pub fn open_read_only(
        path: impl AsRef<Path>,
        config: &DirectIoConfig,
    ) -> Result<Self, DirectIoError> {
        config.validate()?;

        let (file, direct_io_enabled) = Self::open_read_only_with_direct_io(path.as_ref(), config)?;

        Ok(Self {
            file,
            config: config.clone(),
            write_count: 0,
            direct_io_enabled,
        })
    }

    /// 플랫폼별 Direct I/O 파일 열기
    #[cfg(target_os = "linux")]
    fn open_with_direct_io(
        path: &Path,
        config: &DirectIoConfig,
    ) -> Result<(File, bool), DirectIoError> {
        if config.enabled {
            // Linux: O_DIRECT 플래그 사용
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .custom_flags(libc::O_DIRECT)
                .open(path)
            {
                Ok(file) => return Ok((file, true)),
                Err(e) => {
                    // O_DIRECT 실패 시 일반 모드로 폴백
                    eprintln!(
                        "Direct I/O not supported, falling back to buffered I/O: {}",
                        e
                    );
                }
            }
        }

        // 일반 모드로 열기
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        Ok((file, false))
    }

    /// macOS에서 Direct I/O 열기
    #[cfg(target_os = "macos")]
    fn open_with_direct_io(
        path: &Path,
        config: &DirectIoConfig,
    ) -> Result<(File, bool), DirectIoError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        if config.enabled {
            // macOS: F_NOCACHE 사용
            let fd = file.as_raw_fd();
            let result = unsafe { libc::fcntl(fd, libc::F_NOCACHE, 1) };

            if result == -1 {
                eprintln!("F_NOCACHE failed, using buffered I/O");
                return Ok((file, false));
            }

            return Ok((file, true));
        }

        Ok((file, false))
    }

    /// 기타 플랫폼 (Windows 등)
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn open_with_direct_io(
        path: &Path,
        _config: &DirectIoConfig,
    ) -> Result<(File, bool), DirectIoError> {
        // 다른 플랫폼은 일반 I/O 사용
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        Ok((file, false))
    }

    /// 읽기 전용 열기 (Linux)
    #[cfg(target_os = "linux")]
    fn open_read_only_with_direct_io(
        path: &Path,
        config: &DirectIoConfig,
    ) -> Result<(File, bool), DirectIoError> {
        if config.enabled
            && let Ok(file) = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECT)
                .open(path)
        {
            return Ok((file, true));
        }

        let file = OpenOptions::new().read(true).open(path)?;
        Ok((file, false))
    }

    /// 읽기 전용 열기 (macOS)
    #[cfg(target_os = "macos")]
    fn open_read_only_with_direct_io(
        path: &Path,
        config: &DirectIoConfig,
    ) -> Result<(File, bool), DirectIoError> {
        let file = OpenOptions::new().read(true).open(path)?;

        if config.enabled {
            let fd = file.as_raw_fd();
            let result = unsafe { libc::fcntl(fd, libc::F_NOCACHE, 1) };
            if result != -1 {
                return Ok((file, true));
            }
        }

        Ok((file, false))
    }

    /// 읽기 전용 열기 (기타 플랫폼)
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn open_read_only_with_direct_io(
        path: &Path,
        _config: &DirectIoConfig,
    ) -> Result<(File, bool), DirectIoError> {
        let file = OpenOptions::new().read(true).open(path)?;
        Ok((file, false))
    }

    /// Direct I/O 활성화 여부
    pub fn is_direct_io_enabled(&self) -> bool {
        self.direct_io_enabled
    }

    /// 설정 조회
    pub fn config(&self) -> &DirectIoConfig {
        &self.config
    }

    /// 특정 오프셋에서 읽기
    pub fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<usize, DirectIoError> {
        // 정렬 검사 (Direct I/O 모드에서만)
        if self.direct_io_enabled {
            Self::check_alignment(offset, buffer.len(), self.config.alignment)?;
        }

        self.file.seek(SeekFrom::Start(offset))?;
        let bytes_read = self.file.read(buffer)?;

        Ok(bytes_read)
    }

    /// 특정 오프셋에 쓰기
    pub fn write_at(&mut self, offset: u64, buffer: &[u8]) -> Result<usize, DirectIoError> {
        // 정렬 검사 (Direct I/O 모드에서만)
        if self.direct_io_enabled {
            Self::check_alignment(offset, buffer.len(), self.config.alignment)?;
        }

        self.file.seek(SeekFrom::Start(offset))?;
        let bytes_written = self.file.write(buffer)?;

        self.write_count += 1;

        // fsync 간격에 따라 동기화
        if self.config.fsync_interval > 0
            && self.write_count.is_multiple_of(self.config.fsync_interval)
        {
            self.sync()?;
        }

        Ok(bytes_written)
    }

    /// 정렬된 버퍼로 읽기
    pub fn read_aligned(
        &mut self,
        offset: u64,
        buffer: &mut AlignedBuffer,
    ) -> Result<usize, DirectIoError> {
        self.read_at(offset, buffer.as_mut_slice())
    }

    /// 정렬된 버퍼로 쓰기
    pub fn write_aligned(
        &mut self,
        offset: u64,
        buffer: &AlignedBuffer,
    ) -> Result<usize, DirectIoError> {
        self.write_at(offset, buffer.as_slice())
    }

    /// 페이지 읽기
    pub fn read_page(
        &mut self,
        page_id: u32,
        buffer: &mut AlignedBuffer,
    ) -> Result<usize, DirectIoError> {
        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.read_aligned(offset, buffer)
    }

    /// 페이지 쓰기
    pub fn write_page(
        &mut self,
        page_id: u32,
        buffer: &AlignedBuffer,
    ) -> Result<usize, DirectIoError> {
        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.write_aligned(offset, buffer)
    }

    /// 파일 동기화
    pub fn sync(&mut self) -> Result<(), DirectIoError> {
        self.file.sync_all()?;
        Ok(())
    }

    /// 파일 크기
    pub fn file_size(&self) -> Result<u64, DirectIoError> {
        Ok(self.file.metadata()?.len())
    }

    /// 파일 크기 설정 (truncate/extend)
    pub fn set_len(&mut self, size: u64) -> Result<(), DirectIoError> {
        self.file.set_len(size)?;
        Ok(())
    }

    /// 정렬 검사
    fn check_alignment(offset: u64, len: usize, alignment: usize) -> Result<(), DirectIoError> {
        if !(offset as usize).is_multiple_of(alignment) {
            return Err(DirectIoError::UnalignedOffset { offset, alignment });
        }

        if !len.is_multiple_of(alignment) {
            return Err(DirectIoError::UnalignedLength {
                length: len,
                alignment,
            });
        }

        Ok(())
    }
}

// ============================================================================
// Direct I/O 디스크 매니저
// ============================================================================

/// Direct I/O를 지원하는 디스크 매니저
#[derive(Debug)]
pub struct DirectIoDiskManager {
    file: std::sync::Mutex<DirectIoFile>,
    config: DirectIoConfig,
    num_reads: std::sync::atomic::AtomicU64,
    num_writes: std::sync::atomic::AtomicU64,
}

impl DirectIoDiskManager {
    /// 새로운 Direct I/O 디스크 매니저 생성
    pub fn new(path: impl AsRef<Path>, config: DirectIoConfig) -> Result<Self, DirectIoError> {
        let file = DirectIoFile::open(path, &config)?;

        Ok(Self {
            file: std::sync::Mutex::new(file),
            config,
            num_reads: std::sync::atomic::AtomicU64::new(0),
            num_writes: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// 기본 설정으로 생성
    pub fn with_default_config(path: impl AsRef<Path>) -> Result<Self, DirectIoError> {
        Self::new(path, DirectIoConfig::default())
    }

    /// Direct I/O 활성화 여부
    pub fn is_direct_io_enabled(&self) -> bool {
        self.file
            .lock()
            .map(|f| f.is_direct_io_enabled())
            .unwrap_or(false)
    }

    /// 페이지 읽기
    pub fn read_page(&self, page_id: u32) -> Result<AlignedBuffer, DirectIoError> {
        let mut file = self.file.lock().map_err(|_| DirectIoError::LockError)?;
        let mut buffer = AlignedBuffer::new(PAGE_SIZE, self.config.alignment)?;

        file.read_page(page_id, &mut buffer)?;

        self.num_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(buffer)
    }

    /// 페이지 쓰기
    pub fn write_page(&self, page_id: u32, data: &[u8]) -> Result<(), DirectIoError> {
        if data.len() != PAGE_SIZE {
            return Err(DirectIoError::InvalidPageSize {
                expected: PAGE_SIZE,
                actual: data.len(),
            });
        }

        let mut buffer = AlignedBuffer::new(PAGE_SIZE, self.config.alignment)?;
        buffer.copy_from(data);

        let mut file = self.file.lock().map_err(|_| DirectIoError::LockError)?;
        file.write_page(page_id, &buffer)?;

        self.num_writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(())
    }

    /// 새 페이지 할당
    pub fn allocate_page(&self) -> Result<u32, DirectIoError> {
        let mut file = self.file.lock().map_err(|_| DirectIoError::LockError)?;
        let size = file.file_size()?;

        if size % PAGE_SIZE as u64 != 0 {
            return Err(DirectIoError::CorruptedFile {
                message: "File size not aligned to page size".to_string(),
            });
        }

        let page_id = (size / PAGE_SIZE as u64) as u32;

        // 새 페이지 공간 확보
        let new_size = size + PAGE_SIZE as u64;
        file.set_len(new_size)?;

        // 페이지 초기화 (0으로 채움)
        let buffer = AlignedBuffer::new(PAGE_SIZE, self.config.alignment)?;
        file.write_page(page_id, &buffer)?;

        Ok(page_id)
    }

    /// 파일 동기화
    pub fn sync(&self) -> Result<(), DirectIoError> {
        let mut file = self.file.lock().map_err(|_| DirectIoError::LockError)?;
        file.sync()
    }

    /// 파일 크기
    pub fn file_size(&self) -> Result<u64, DirectIoError> {
        let file = self.file.lock().map_err(|_| DirectIoError::LockError)?;
        file.file_size()
    }

    /// 통계 조회
    pub fn stats(&self) -> DirectIoStats {
        DirectIoStats {
            num_reads: self.num_reads.load(std::sync::atomic::Ordering::Relaxed),
            num_writes: self.num_writes.load(std::sync::atomic::Ordering::Relaxed),
            direct_io_enabled: self.is_direct_io_enabled(),
            alignment: self.config.alignment,
        }
    }
}

/// Direct I/O 통계
#[derive(Debug, Clone)]
pub struct DirectIoStats {
    /// 읽기 횟수
    pub num_reads: u64,
    /// 쓰기 횟수
    pub num_writes: u64,
    /// Direct I/O 활성화 여부
    pub direct_io_enabled: bool,
    /// 정렬 크기
    pub alignment: usize,
}

// ============================================================================
// 에러 타입
// ============================================================================

/// Direct I/O 에러
#[derive(Debug)]
pub enum DirectIoError {
    /// I/O 에러
    Io(io::Error),
    /// 잘못된 정렬 크기
    InvalidAlignment { alignment: usize, min: usize },
    /// 정렬이 2의 거듭제곱이 아님
    AlignmentNotPowerOfTwo { alignment: usize },
    /// 정렬되지 않은 오프셋
    UnalignedOffset { offset: u64, alignment: usize },
    /// 정렬되지 않은 길이
    UnalignedLength { length: usize, alignment: usize },
    /// 잘못된 페이지 크기
    InvalidPageSize { expected: usize, actual: usize },
    /// 잘못된 버퍼 크기
    InvalidBufferSize { size: usize },
    /// 버퍼 크기와 정렬의 합이 표현 범위를 초과함
    AllocationSizeOverflow { size: usize, alignment: usize },
    /// 정렬을 만족하는 주소를 계산할 수 없음
    UnsatisfiableAlignment { alignment: usize },
    /// 버퍼 할당 실패
    AllocationFailed { message: String },
    /// 손상된 파일
    CorruptedFile { message: String },
    /// 락 에러
    LockError,
}

impl std::fmt::Display for DirectIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::InvalidAlignment { alignment, min } => {
                write!(f, "Invalid alignment: {} (minimum: {})", alignment, min)
            }
            Self::AlignmentNotPowerOfTwo { alignment } => {
                write!(f, "Alignment {} is not a power of two", alignment)
            }
            Self::UnalignedOffset { offset, alignment } => {
                write!(f, "Offset {} is not aligned to {}", offset, alignment)
            }
            Self::UnalignedLength { length, alignment } => {
                write!(f, "Length {} is not aligned to {}", length, alignment)
            }
            Self::InvalidPageSize { expected, actual } => {
                write!(
                    f,
                    "Invalid page size: expected {}, got {}",
                    expected, actual
                )
            }
            Self::InvalidBufferSize { size } => {
                write!(f, "Invalid buffer size: {}", size)
            }
            Self::AllocationSizeOverflow { size, alignment } => write!(
                f,
                "Aligned buffer allocation size overflow: size {}, alignment {}",
                size, alignment
            ),
            Self::UnsatisfiableAlignment { alignment } => {
                write!(f, "Allocation cannot satisfy alignment {}", alignment)
            }
            Self::AllocationFailed { message } => {
                write!(f, "Aligned buffer allocation failed: {}", message)
            }
            Self::CorruptedFile { message } => write!(f, "Corrupted file: {}", message),
            Self::LockError => write!(f, "Lock acquisition failed"),
        }
    }
}

impl std::error::Error for DirectIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for DirectIoError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// ============================================================================
// 테스트
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_buffer_exposes_only_requested_aligned_bytes() -> Result<(), DirectIoError> {
        let mut buffer = AlignedBuffer::new(1024, 512)?;

        assert_eq!(buffer.len(), 1024);
        assert!(buffer.is_aligned());
        buffer.as_mut_slice().fill(7);
        assert!(buffer.as_slice().iter().all(|byte| *byte == 7));
        Ok(())
    }

    #[test]
    fn aligned_buffer_rejects_invalid_inputs() {
        assert!(matches!(
            AlignedBuffer::new(0, 512),
            Err(DirectIoError::InvalidBufferSize { size: 0 })
        ));
        assert!(matches!(
            AlignedBuffer::new(1024, 513),
            Err(DirectIoError::AlignmentNotPowerOfTwo { alignment: 513 })
        ));
        assert!(matches!(
            AlignedBuffer::new(usize::MAX, 512),
            Err(DirectIoError::AllocationSizeOverflow {
                size: usize::MAX,
                alignment: 512
            })
        ));
    }
}
