// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! IO 압축 지원
//!
//! v1.57.0: gzip/lz4 압축 지원
//!
//! 그래프 데이터 파일의 압축 및 압축 해제를 지원합니다.
//!
//! # 지원 포맷
//!
//! - **Gzip**: 높은 압축률, 범용 호환성
//! - **Lz4**: 빠른 압축/해제 속도, 적당한 압축률
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::io::compress::{CompressionFormat, CompressedWriter, CompressedReader};
//!
//! // 압축된 파일로 내보내기
//! let writer = CompressedWriter::new("graph.json.gz", CompressionFormat::Gzip)?;
//!
//! // 압축된 파일 읽기 (자동 감지)
//! let reader = CompressedReader::open("graph.json.gz")?;
//!
//! // 파일 확장자로 압축 포맷 감지
//! let format = CompressionFormat::from_path("data.ndjson.lz4");
//! ```

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

/// 압축 포맷
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionFormat {
    /// 압축 없음
    #[default]
    None,
    /// Gzip 압축 (.gz)
    Gzip,
    /// LZ4 압축 (.lz4)
    Lz4,
}

impl CompressionFormat {
    /// 파일 경로에서 압축 포맷 감지
    pub fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "gz" | "gzip" => Self::Gzip,
            "lz4" => Self::Lz4,
            _ => Self::None,
        }
    }

    /// 문자열에서 압축 포맷 파싱
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "gzip" | "gz" => Self::Gzip,
            "lz4" => Self::Lz4,
            "none" | "" => Self::None,
            _ => Self::None,
        }
    }

    /// 압축 포맷을 문자열로
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Gzip => "gzip",
            Self::Lz4 => "lz4",
        }
    }

    /// 파일 확장자
    pub fn extension(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::Gzip => ".gz",
            Self::Lz4 => ".lz4",
        }
    }

    /// 압축 포맷인지 확인
    pub fn is_compressed(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Gzip 압축 레벨
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GzipLevel {
    /// 빠른 압축 (레벨 1)
    Fast,
    /// 기본 압축 (레벨 6)
    #[default]
    Default,
    /// 최고 압축 (레벨 9)
    Best,
    /// 사용자 정의 레벨 (0-9)
    Custom(u32),
}

impl GzipLevel {
    fn to_flate2(self) -> Compression {
        match self {
            Self::Fast => Compression::fast(),
            Self::Default => Compression::default(),
            Self::Best => Compression::best(),
            Self::Custom(level) => Compression::new(level),
        }
    }
}

/// LZ4 압축 레벨
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lz4Level {
    /// 빠른 압축 (레벨 1)
    Fast,
    /// 기본 압축 (레벨 4)
    #[default]
    Default,
    /// 높은 압축 (레벨 9)
    High,
    /// 사용자 정의 레벨 (1-16)
    Custom(u32),
}

impl Lz4Level {
    fn to_level(self) -> u32 {
        match self {
            Self::Fast => 1,
            Self::Default => 4,
            Self::High => 9,
            Self::Custom(level) => level,
        }
    }
}

/// 압축 옵션
#[derive(Debug, Clone)]
pub struct CompressionOptions {
    /// 압축 포맷
    pub format: CompressionFormat,
    /// Gzip 압축 레벨
    pub gzip_level: GzipLevel,
    /// LZ4 압축 레벨
    pub lz4_level: Lz4Level,
    /// 버퍼 크기
    pub buffer_size: usize,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            format: CompressionFormat::None,
            gzip_level: GzipLevel::Default,
            lz4_level: Lz4Level::Default,
            buffer_size: 64 * 1024, // 64KB
        }
    }
}

impl CompressionOptions {
    /// 새 압축 옵션 생성
    pub fn new(format: CompressionFormat) -> Self {
        Self {
            format,
            ..Default::default()
        }
    }

    /// Gzip 압축 옵션
    pub fn gzip() -> Self {
        Self::new(CompressionFormat::Gzip)
    }

    /// LZ4 압축 옵션
    pub fn lz4() -> Self {
        Self::new(CompressionFormat::Lz4)
    }

    /// Gzip 레벨 설정
    pub fn with_gzip_level(mut self, level: GzipLevel) -> Self {
        self.gzip_level = level;
        self
    }

    /// LZ4 레벨 설정
    pub fn with_lz4_level(mut self, level: Lz4Level) -> Self {
        self.lz4_level = level;
        self
    }

    /// 버퍼 크기 설정
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }
}

/// 압축된 Writer 열거형
enum CompressedWriterInner {
    Plain(BufWriter<File>),
    Gzip(GzEncoder<BufWriter<File>>),
    Lz4(lz4::Encoder<BufWriter<File>>),
}

/// 압축된 파일 Writer
pub struct CompressedWriter {
    inner: CompressedWriterInner,
    format: CompressionFormat,
}

impl CompressedWriter {
    /// 파일 생성 (경로에서 압축 포맷 자동 감지)
    pub fn create<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let format = CompressionFormat::from_path(&path);
        Self::create_with_format(path, format)
    }

    /// 압축 옵션으로 파일 생성
    pub fn create_with_options<P: AsRef<Path>>(
        path: P,
        options: &CompressionOptions,
    ) -> io::Result<Self> {
        let file = File::create(path)?;
        let buf_writer = BufWriter::with_capacity(options.buffer_size, file);

        let inner = match options.format {
            CompressionFormat::None => CompressedWriterInner::Plain(buf_writer),
            CompressionFormat::Gzip => {
                let encoder = GzEncoder::new(buf_writer, options.gzip_level.to_flate2());
                CompressedWriterInner::Gzip(encoder)
            }
            CompressionFormat::Lz4 => {
                let builder = lz4::EncoderBuilder::new()
                    .level(options.lz4_level.to_level())
                    .build(buf_writer)?;
                CompressedWriterInner::Lz4(builder)
            }
        };

        Ok(Self {
            inner,
            format: options.format,
        })
    }

    /// 압축 포맷 지정하여 파일 생성
    pub fn create_with_format<P: AsRef<Path>>(
        path: P,
        format: CompressionFormat,
    ) -> io::Result<Self> {
        let options = CompressionOptions::new(format);
        Self::create_with_options(path, &options)
    }

    /// 압축 포맷 조회
    pub fn format(&self) -> CompressionFormat {
        self.format
    }

    /// 완료 및 플러시
    pub fn finish(self) -> io::Result<()> {
        match self.inner {
            CompressedWriterInner::Plain(mut w) => w.flush(),
            CompressedWriterInner::Gzip(encoder) => encoder.finish()?.flush(),
            CompressedWriterInner::Lz4(encoder) => {
                let (mut w, result) = encoder.finish();
                result?;
                w.flush()
            }
        }
    }
}

impl Write for CompressedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.inner {
            CompressedWriterInner::Plain(w) => w.write(buf),
            CompressedWriterInner::Gzip(w) => w.write(buf),
            CompressedWriterInner::Lz4(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.inner {
            CompressedWriterInner::Plain(w) => w.flush(),
            CompressedWriterInner::Gzip(w) => w.flush(),
            CompressedWriterInner::Lz4(w) => w.flush(),
        }
    }
}

/// 압축된 Reader 열거형
enum CompressedReaderInner {
    Plain(BufReader<File>),
    Gzip(BufReader<GzDecoder<File>>),
    Lz4(BufReader<lz4::Decoder<File>>),
}

/// 압축된 파일 Reader
pub struct CompressedReader {
    inner: CompressedReaderInner,
    format: CompressionFormat,
}

impl CompressedReader {
    /// 파일 열기 (경로에서 압축 포맷 자동 감지)
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let format = CompressionFormat::from_path(&path);
        Self::open_with_format(path, format)
    }

    /// 압축 포맷 지정하여 파일 열기
    pub fn open_with_format<P: AsRef<Path>>(
        path: P,
        format: CompressionFormat,
    ) -> io::Result<Self> {
        let options = CompressionOptions::new(format);
        Self::open_with_options(path, &options)
    }

    /// 압축 옵션으로 파일 열기
    pub fn open_with_options<P: AsRef<Path>>(
        path: P,
        options: &CompressionOptions,
    ) -> io::Result<Self> {
        let file = File::open(path)?;

        let inner = match options.format {
            CompressionFormat::None => {
                let buf_reader = BufReader::with_capacity(options.buffer_size, file);
                CompressedReaderInner::Plain(buf_reader)
            }
            CompressionFormat::Gzip => {
                let decoder = GzDecoder::new(file);
                let buf_reader = BufReader::with_capacity(options.buffer_size, decoder);
                CompressedReaderInner::Gzip(buf_reader)
            }
            CompressionFormat::Lz4 => {
                let decoder = lz4::Decoder::new(file)?;
                let buf_reader = BufReader::with_capacity(options.buffer_size, decoder);
                CompressedReaderInner::Lz4(buf_reader)
            }
        };

        Ok(Self {
            inner,
            format: options.format,
        })
    }

    /// 압축 포맷 조회
    pub fn format(&self) -> CompressionFormat {
        self.format
    }
}

impl Read for CompressedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            CompressedReaderInner::Plain(r) => r.read(buf),
            CompressedReaderInner::Gzip(r) => r.read(buf),
            CompressedReaderInner::Lz4(r) => r.read(buf),
        }
    }
}

impl BufRead for CompressedReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match &mut self.inner {
            CompressedReaderInner::Plain(r) => r.fill_buf(),
            CompressedReaderInner::Gzip(r) => r.fill_buf(),
            CompressedReaderInner::Lz4(r) => r.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match &mut self.inner {
            CompressedReaderInner::Plain(r) => r.consume(amt),
            CompressedReaderInner::Gzip(r) => r.consume(amt),
            CompressedReaderInner::Lz4(r) => r.consume(amt),
        }
    }
}

/// 압축 통계
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    /// 원본 크기 (바이트)
    pub original_size: u64,
    /// 압축 후 크기 (바이트)
    pub compressed_size: u64,
    /// 압축 포맷
    pub format: CompressionFormat,
}

impl CompressionStats {
    /// 압축률 계산 (0.0 ~ 1.0)
    pub fn ratio(&self) -> f64 {
        if self.original_size == 0 {
            0.0
        } else {
            self.compressed_size as f64 / self.original_size as f64
        }
    }

    /// 공간 절약률 (0.0 ~ 1.0)
    pub fn savings(&self) -> f64 {
        1.0 - self.ratio()
    }

    /// 압축률 백분율 문자열
    pub fn ratio_percent(&self) -> String {
        format!("{:.1}%", self.ratio() * 100.0)
    }

    /// 공간 절약 백분율 문자열
    pub fn savings_percent(&self) -> String {
        format!("{:.1}%", self.savings() * 100.0)
    }
}

/// 데이터를 압축
pub fn compress_data(data: &[u8], format: CompressionFormat) -> io::Result<Vec<u8>> {
    match format {
        CompressionFormat::None => Ok(data.to_vec()),
        CompressionFormat::Gzip => {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(data)?;
            encoder.finish()
        }
        CompressionFormat::Lz4 => {
            let mut output = Vec::new();
            let mut encoder = lz4::EncoderBuilder::new().build(&mut output)?;
            encoder.write_all(data)?;
            let (_, result) = encoder.finish();
            result?;
            Ok(output)
        }
    }
}

/// 데이터를 압축 해제
pub fn decompress_data(data: &[u8], format: CompressionFormat) -> io::Result<Vec<u8>> {
    match format {
        CompressionFormat::None => Ok(data.to_vec()),
        CompressionFormat::Gzip => {
            let mut decoder = GzDecoder::new(data);
            let mut output = Vec::new();
            decoder.read_to_end(&mut output)?;
            Ok(output)
        }
        CompressionFormat::Lz4 => {
            let mut decoder = lz4::Decoder::new(data)?;
            let mut output = Vec::new();
            decoder.read_to_end(&mut output)?;
            Ok(output)
        }
    }
}

/// 파일 압축 (원본 파일 → 압축 파일)
pub fn compress_file<P: AsRef<Path>, Q: AsRef<Path>>(
    source: P,
    dest: Q,
    options: &CompressionOptions,
) -> io::Result<CompressionStats> {
    let source_path = source.as_ref();
    let original_size = std::fs::metadata(source_path)?.len();

    let mut reader = BufReader::new(File::open(source_path)?);
    let mut writer = CompressedWriter::create_with_options(&dest, options)?;

    io::copy(&mut reader, &mut writer)?;
    writer.finish()?;

    let compressed_size = std::fs::metadata(dest.as_ref())?.len();

    Ok(CompressionStats {
        original_size,
        compressed_size,
        format: options.format,
    })
}

/// 파일 압축 해제 (압축 파일 → 원본 파일)
pub fn decompress_file<P: AsRef<Path>, Q: AsRef<Path>>(
    source: P,
    dest: Q,
) -> io::Result<CompressionStats> {
    let source_path = source.as_ref();
    let compressed_size = std::fs::metadata(source_path)?.len();
    let format = CompressionFormat::from_path(source_path);

    let mut reader = CompressedReader::open(source_path)?;
    let mut writer = BufWriter::new(File::create(&dest)?);

    io::copy(&mut reader, &mut writer)?;
    writer.flush()?;

    let original_size = std::fs::metadata(dest.as_ref())?.len();

    Ok(CompressionStats {
        original_size,
        compressed_size,
        format,
    })
}
