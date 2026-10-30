//! M9.6: Reactive Streams
//!
//! tokio-stream 기반 비동기 리액티브 스트림
//!
//! # Example
//!
//! ```ignore
//! use zeta4j_server::driver::reactive::{ReactiveRecordStream, StreamExt};
//!
//! let stream = session.run_reactive("MATCH (n) RETURN n", None).await?;
//!
//! // 스트림 변환 및 필터링
//! let filtered = stream
//!     .filter(|r| r.get_int("age").map(|a| a > 18).unwrap_or(false))
//!     .map(|r| r.get_string("name").unwrap_or_default())
//!     .take(10);
//!
//! // 비동기 수집
//! let names: Vec<String> = filtered.collect().await;
//! ```

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Stream;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::error::{DriverError, DriverResult};
use super::record::Record;

// ============================================================================
// ReactiveRecordStream - 비동기 레코드 스트림
// ============================================================================

/// 비동기 레코드 스트림
///
/// `tokio-stream` 기반 비동기 반복자입니다.
/// `Stream` 트레이트를 구현하여 async/await와 함께 사용할 수 있습니다.
pub struct ReactiveRecordStream {
    inner: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>,
    keys: Option<Vec<String>>,
}

impl std::fmt::Debug for ReactiveRecordStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReactiveRecordStream")
            .field("keys", &self.keys)
            .finish()
    }
}

impl ReactiveRecordStream {
    /// 레코드 벡터에서 생성
    pub fn from_records(records: Vec<Record>) -> Self {
        let keys = records.first().map(|r| r.keys().to_vec());
        let iter = records.into_iter().map(Ok);
        Self {
            inner: Box::pin(tokio_stream::iter(iter)),
            keys,
        }
    }

    /// 빈 스트림 생성
    pub fn empty() -> Self {
        Self {
            inner: Box::pin(tokio_stream::empty()),
            keys: None,
        }
    }

    /// mpsc 채널에서 생성
    pub fn from_channel(
        rx: mpsc::Receiver<DriverResult<Record>>,
        keys: Option<Vec<String>>,
    ) -> Self {
        Self {
            inner: Box::pin(ReceiverStream::new(rx)),
            keys,
        }
    }

    /// 에러 스트림 생성
    pub fn error(err: DriverError) -> Self {
        Self {
            inner: Box::pin(tokio_stream::once(Err(err))),
            keys: None,
        }
    }

    /// 키 목록
    pub fn keys(&self) -> Option<&[String]> {
        self.keys.as_deref()
    }

    /// 모든 레코드 수집 (성공한 것만)
    pub async fn collect(self) -> Vec<Record> {
        use tokio_stream::StreamExt;

        self.inner.filter_map(|r| r.ok()).collect().await
    }

    /// 모든 레코드 수집 (에러 포함)
    pub async fn try_collect(self) -> DriverResult<Vec<Record>> {
        use tokio_stream::StreamExt;

        let mut results = Vec::new();
        let mut stream = self.inner;

        while let Some(result) = stream.next().await {
            results.push(result?);
        }

        Ok(results)
    }

    /// 첫 번째 레코드
    pub async fn first(self) -> Option<Record> {
        use tokio_stream::StreamExt;

        self.inner.filter_map(|r| r.ok()).next().await
    }

    /// 단일 레코드 (정확히 1개)
    pub async fn single(self) -> DriverResult<Record> {
        use tokio_stream::StreamExt;

        let records: Vec<_> = self.inner.filter_map(|r| r.ok()).take(2).collect().await;

        if records.len() != 1 {
            return Err(DriverError::type_conversion(format!(
                "Expected single record, got {}",
                records.len()
            )));
        }

        records
            .into_iter()
            .next()
            .ok_or_else(|| DriverError::type_conversion("Expected a single record"))
    }

    /// 스트림 매핑
    pub fn map<F, T>(self, f: F) -> MappedStream<T>
    where
        F: FnMut(Record) -> T + Send + 'static,
        T: Send + 'static,
    {
        MappedStream::new(self.inner, f)
    }

    /// 스트림 필터링
    pub fn filter<F>(self, predicate: F) -> FilteredStream
    where
        F: FnMut(&Record) -> bool + Send + 'static,
    {
        FilteredStream::new(self.inner, predicate)
    }

    /// 스트림 제한
    pub fn take(self, n: usize) -> TakeStream {
        TakeStream::new(self.inner, n)
    }

    /// 스트림 스킵
    pub fn skip(self, n: usize) -> SkipStream {
        SkipStream::new(self.inner, n)
    }

    /// 스트림 청크 분할
    pub fn chunks(self, size: usize) -> ChunkedStream {
        ChunkedStream::new(self.inner, size)
    }

    /// 스트림 버퍼링
    pub fn buffered(self, capacity: usize) -> BufferedStream {
        BufferedStream::new(self.inner, capacity)
    }

    /// for_each 실행
    pub async fn for_each<F, Fut>(self, mut f: F)
    where
        F: FnMut(Record) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        use tokio_stream::StreamExt;

        let mut stream = self.inner;
        while let Some(Ok(record)) = stream.next().await {
            f(record).await;
        }
    }

    /// 레코드 수 카운트
    pub async fn count(self) -> usize {
        use tokio_stream::StreamExt;

        let mut count = 0usize;
        let mut stream = self.inner;
        while let Some(result) = stream.next().await {
            if result.is_ok() {
                count += 1;
            }
        }
        count
    }
}

impl Stream for ReactiveRecordStream {
    type Item = DriverResult<Record>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

// ============================================================================
// MappedStream - 매핑된 스트림
// ============================================================================

/// 매핑된 스트림
pub struct MappedStream<T> {
    inner: Pin<Box<dyn Stream<Item = T> + Send>>,
}

impl<T: Send + 'static> MappedStream<T> {
    fn new<F>(source: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>, mut f: F) -> Self
    where
        F: FnMut(Record) -> T + Send + 'static,
    {
        let mapped = tokio_stream::StreamExt::filter_map(source, move |r| r.ok().map(&mut f));
        Self {
            inner: Box::pin(mapped),
        }
    }

    /// 모든 값 수집
    pub async fn collect(self) -> Vec<T> {
        use tokio_stream::StreamExt;
        self.inner.collect().await
    }

    /// 추가 매핑
    pub fn map<F2, U>(self, f: F2) -> MappedStream<U>
    where
        F2: FnMut(T) -> U + Send + 'static,
        U: Send + 'static,
    {
        let mapped = tokio_stream::StreamExt::map(self.inner, f);
        MappedStream {
            inner: Box::pin(mapped),
        }
    }

    /// 필터링
    pub fn filter<F2>(self, predicate: F2) -> MappedStream<T>
    where
        F2: FnMut(&T) -> bool + Send + 'static,
    {
        let filtered = tokio_stream::StreamExt::filter(self.inner, predicate);
        MappedStream {
            inner: Box::pin(filtered),
        }
    }

    /// 제한
    pub fn take(self, n: usize) -> MappedStream<T> {
        let taken = tokio_stream::StreamExt::take(self.inner, n);
        MappedStream {
            inner: Box::pin(taken),
        }
    }
}

impl<T> Stream for MappedStream<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

// ============================================================================
// FilteredStream - 필터링된 스트림
// ============================================================================

/// 필터링된 스트림
pub struct FilteredStream {
    inner: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>,
}

impl FilteredStream {
    fn new<F>(
        source: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>,
        mut predicate: F,
    ) -> Self
    where
        F: FnMut(&Record) -> bool + Send + 'static,
    {
        let filtered = tokio_stream::StreamExt::filter(source, move |r| {
            r.as_ref().map(&mut predicate).unwrap_or(true)
        });
        Self {
            inner: Box::pin(filtered),
        }
    }

    /// 모든 레코드 수집
    pub async fn collect(self) -> Vec<Record> {
        use tokio_stream::StreamExt;
        self.inner.filter_map(|r| r.ok()).collect().await
    }

    /// 매핑
    pub fn map<F, T>(self, f: F) -> MappedStream<T>
    where
        F: FnMut(Record) -> T + Send + 'static,
        T: Send + 'static,
    {
        MappedStream::new(self.inner, f)
    }

    /// 추가 필터링
    pub fn filter<F>(self, predicate: F) -> FilteredStream
    where
        F: FnMut(&Record) -> bool + Send + 'static,
    {
        FilteredStream::new(self.inner, predicate)
    }

    /// 제한
    pub fn take(self, n: usize) -> TakeStream {
        TakeStream::new(self.inner, n)
    }
}

impl Stream for FilteredStream {
    type Item = DriverResult<Record>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

// ============================================================================
// TakeStream - 제한된 스트림
// ============================================================================

/// 제한된 스트림
pub struct TakeStream {
    inner: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>,
}

impl TakeStream {
    fn new(source: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>, n: usize) -> Self {
        let taken = tokio_stream::StreamExt::take(source, n);
        Self {
            inner: Box::pin(taken),
        }
    }

    /// 모든 레코드 수집
    pub async fn collect(self) -> Vec<Record> {
        use tokio_stream::StreamExt;
        self.inner.filter_map(|r| r.ok()).collect().await
    }

    /// 매핑
    pub fn map<F, T>(self, f: F) -> MappedStream<T>
    where
        F: FnMut(Record) -> T + Send + 'static,
        T: Send + 'static,
    {
        MappedStream::new(self.inner, f)
    }
}

impl Stream for TakeStream {
    type Item = DriverResult<Record>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

// ============================================================================
// SkipStream - 건너뛰기 스트림
// ============================================================================

/// 건너뛰기 스트림
pub struct SkipStream {
    inner: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>,
}

impl SkipStream {
    fn new(source: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>, n: usize) -> Self {
        let skipped = tokio_stream::StreamExt::skip(source, n);
        Self {
            inner: Box::pin(skipped),
        }
    }

    /// 모든 레코드 수집
    pub async fn collect(self) -> Vec<Record> {
        use tokio_stream::StreamExt;
        self.inner.filter_map(|r| r.ok()).collect().await
    }

    /// 제한
    pub fn take(self, n: usize) -> TakeStream {
        TakeStream::new(self.inner, n)
    }
}

impl Stream for SkipStream {
    type Item = DriverResult<Record>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

// ============================================================================
// ChunkedStream - 청크 스트림
// ============================================================================

/// 청크 스트림
pub struct ChunkedStream {
    source: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>,
    chunk_size: usize,
    buffer: Vec<Record>,
}

impl ChunkedStream {
    fn new(
        source: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>,
        chunk_size: usize,
    ) -> Self {
        Self {
            source,
            chunk_size: chunk_size.max(1),
            buffer: Vec::with_capacity(chunk_size),
        }
    }

    /// 모든 청크 수집
    pub async fn collect(mut self) -> Vec<Vec<Record>> {
        use tokio_stream::StreamExt;

        let mut chunks = Vec::new();
        while let Some(chunk) = self.next().await {
            chunks.push(chunk);
        }
        chunks
    }
}

impl Stream for ChunkedStream {
    type Item = Vec<Record>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let chunk_size = self.chunk_size;
        loop {
            match self.source.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(record))) => {
                    self.buffer.push(record);
                    if self.buffer.len() >= chunk_size {
                        let chunk = std::mem::take(&mut self.buffer);
                        self.buffer.reserve(chunk_size);
                        return Poll::Ready(Some(chunk));
                    }
                }
                Poll::Ready(Some(Err(_))) => {
                    // 에러는 무시하고 계속
                    continue;
                }
                Poll::Ready(None) => {
                    if self.buffer.is_empty() {
                        return Poll::Ready(None);
                    } else {
                        let chunk = std::mem::take(&mut self.buffer);
                        return Poll::Ready(Some(chunk));
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ============================================================================
// BufferedStream - 버퍼 스트림
// ============================================================================

/// 버퍼 스트림 (백프레셔 지원)
pub struct BufferedStream {
    source: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>,
    buffer: VecDeque<Record>,
    capacity: usize,
    exhausted: bool,
}

impl BufferedStream {
    fn new(
        source: Pin<Box<dyn Stream<Item = DriverResult<Record>> + Send>>,
        capacity: usize,
    ) -> Self {
        Self {
            source,
            buffer: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
            exhausted: false,
        }
    }

    /// 버퍼 길이
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// 버퍼 용량
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 버퍼가 가득 찼는지
    pub fn is_buffer_full(&self) -> bool {
        self.buffer.len() >= self.capacity
    }

    /// 모든 레코드 수집
    pub async fn collect(mut self) -> Vec<Record> {
        use tokio_stream::StreamExt;

        let mut results = Vec::new();
        while let Some(record) = self.next().await {
            results.push(record);
        }
        results
    }

    /// 버퍼 채우기 (내부용)
    fn fill_buffer(&mut self, cx: &mut Context<'_>) {
        while !self.exhausted && self.buffer.len() < self.capacity {
            match self.source.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(record))) => {
                    self.buffer.push_back(record);
                }
                Poll::Ready(Some(Err(_))) => {
                    // 에러는 무시
                    continue;
                }
                Poll::Ready(None) => {
                    self.exhausted = true;
                    break;
                }
                Poll::Pending => break,
            }
        }
    }
}

impl Stream for BufferedStream {
    type Item = Record;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 버퍼 채우기 시도
        self.fill_buffer(cx);

        // 버퍼에서 반환
        if let Some(record) = self.buffer.pop_front() {
            Poll::Ready(Some(record))
        } else if self.exhausted {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

// ============================================================================
// StreamProducer - 스트림 생산자 (채널 기반)
// ============================================================================

/// 스트림 생산자
///
/// 백그라운드 태스크에서 레코드를 생산하고 스트림으로 전달합니다.
pub struct StreamProducer {
    tx: mpsc::Sender<DriverResult<Record>>,
    keys: Arc<Mutex<Option<Vec<String>>>>,
}

impl StreamProducer {
    /// 새 생산자 생성
    pub fn new(buffer_size: usize) -> (Self, ReactiveRecordStream) {
        let (tx, rx) = mpsc::channel(buffer_size);
        let keys = Arc::new(Mutex::new(None));
        let keys_clone = Arc::clone(&keys);

        let producer = Self { tx, keys };

        // 스트림 생성 시 키 설정을 lazy하게 처리
        let stream = ReactiveRecordStream {
            inner: Box::pin(ReceiverStream::new(rx)),
            keys: None, // 나중에 설정됨
        };

        // keys는 나중에 set_keys로 설정
        let _ = keys_clone;

        (producer, stream)
    }

    /// 키 설정
    pub fn set_keys(&self, keys: Vec<String>) {
        *self.keys.lock() = Some(keys);
    }

    /// 레코드 전송
    pub async fn send(&self, record: Record) -> DriverResult<()> {
        self.tx
            .send(Ok(record))
            .await
            .map_err(|_| DriverError::connection("Stream receiver dropped"))
    }

    /// 에러 전송
    pub async fn send_error(&self, error: DriverError) -> DriverResult<()> {
        self.tx
            .send(Err(error))
            .await
            .map_err(|_| DriverError::connection("Stream receiver dropped"))
    }

    /// 레코드 배치 전송
    pub async fn send_batch(&self, records: Vec<Record>) -> DriverResult<()> {
        for record in records {
            self.send(record).await?;
        }
        Ok(())
    }

    /// 용량
    pub fn capacity(&self) -> usize {
        self.tx.capacity()
    }

    /// 닫힘 여부
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}
