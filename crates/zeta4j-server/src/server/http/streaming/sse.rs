// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server-Sent Events Streaming (M2.4)
//!
//! SSE 포맷 스트리밍

use axum::response::sse::Event;
use futures::Stream;
use pin_project_lite::pin_project;
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use super::executor::SharedQueryMemory;
use super::types::{StreamError, StreamFooter, StreamHeader, StreamStats};

/// SSE 이벤트 빌더
pub struct SseEventBuilder;

impl SseEventBuilder {
    /// 헤더 이벤트 생성
    pub fn header_event(columns: &[String], query_id: &str, total_rows: Option<u64>) -> Event {
        let header = StreamHeader {
            columns: columns.to_vec(),
            total_rows,
            query_id: query_id.to_string(),
        };

        Event::default()
            .event("header")
            .data(serde_json::to_string(&header).unwrap_or_default())
    }

    /// 데이터 행 이벤트 생성
    pub fn row_event(row: serde_json::Value, id: u64) -> Event {
        Event::default()
            .event("row")
            .id(id.to_string())
            .data(serde_json::to_string(&row).unwrap_or_default())
    }

    /// 푸터 이벤트 생성
    pub fn footer_event(rows_sent: u64, took_ms: u64, stats: Option<StreamStats>) -> Event {
        let footer = StreamFooter {
            rows_sent,
            took_ms,
            stats,
            bookmark: None,
        };

        Event::default()
            .event("footer")
            .data(serde_json::to_string(&footer).unwrap_or_default())
    }

    /// 에러 이벤트 생성
    pub fn error_event(error: &StreamError) -> Event {
        let error_obj = serde_json::json!({
            "error": error.to_string(),
            "code": error.code(),
            "retryable": error.is_retryable()
        });

        Event::default()
            .event("error")
            .data(serde_json::to_string(&error_obj).unwrap_or_default())
    }

    /// 하트비트 이벤트 생성
    pub fn heartbeat_event() -> Event {
        Event::default()
            .event("heartbeat")
            .data(format!("{}", chrono::Utc::now().timestamp_millis()))
    }

    /// 재시도 간격 설정 이벤트
    pub fn retry_event(retry_ms: u64) -> Event {
        Event::default().retry(Duration::from_millis(retry_ms))
    }
}

/// SSE 스트림 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SseState {
    /// 헤더 전송 전
    Initial,
    /// 데이터 전송 중
    Streaming,
    /// 푸터 전송 필요
    NeedFooter,
    /// 완료
    Done,
}

pin_project! {
    /// SSE 스트림
    pub struct SseStream {
        columns: Vec<String>,
        query_id: String,
        #[pin]
        receiver: mpsc::Receiver<Result<serde_json::Value, StreamError>>,
        state: SseState,
        event_id: u64,
        total_rows: Option<u64>,
        start_time: Instant,
        heartbeat_interval: Option<Duration>,
        last_heartbeat: Instant,
        memory_reservation: Option<SharedQueryMemory>,
    }
}

impl SseStream {
    /// 새 SSE 스트림 생성
    pub fn new(
        columns: Vec<String>,
        query_id: String,
        receiver: mpsc::Receiver<Result<serde_json::Value, StreamError>>,
    ) -> Self {
        let now = Instant::now();
        Self {
            columns,
            query_id,
            receiver,
            state: SseState::Initial,
            event_id: 0,
            total_rows: None,
            start_time: now,
            heartbeat_interval: None,
            last_heartbeat: now,
            memory_reservation: None,
        }
    }

    pub(crate) fn with_memory_reservation(mut self, memory_reservation: SharedQueryMemory) -> Self {
        self.memory_reservation = Some(memory_reservation);
        self
    }

    /// 총 행 수 설정
    pub fn with_total_rows(mut self, total: u64) -> Self {
        self.total_rows = Some(total);
        self
    }

    /// 하트비트 간격 설정
    pub fn with_heartbeat(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = Some(interval);
        self
    }
}

impl Stream for SseStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            match *this.state {
                SseState::Initial => {
                    // 헤더 이벤트 전송
                    let event = SseEventBuilder::header_event(
                        this.columns,
                        this.query_id,
                        *this.total_rows,
                    );
                    *this.state = SseState::Streaming;
                    return Poll::Ready(Some(Ok(event)));
                }

                SseState::Streaming => {
                    // 하트비트 체크
                    if let Some(interval) = this.heartbeat_interval
                        && this.last_heartbeat.elapsed() >= *interval
                    {
                        *this.last_heartbeat = Instant::now();
                        return Poll::Ready(Some(Ok(SseEventBuilder::heartbeat_event())));
                    }

                    // 데이터 수신
                    match this.receiver.as_mut().poll_recv(cx) {
                        Poll::Ready(Some(Ok(row))) => {
                            let id = *this.event_id;
                            *this.event_id += 1;
                            let event = SseEventBuilder::row_event(row, id);
                            return Poll::Ready(Some(Ok(event)));
                        }
                        Poll::Ready(Some(Err(error))) => {
                            let event = SseEventBuilder::error_event(&error);
                            *this.state = SseState::Done;
                            return Poll::Ready(Some(Ok(event)));
                        }
                        Poll::Ready(None) => {
                            // 채널 닫힘 - 푸터 전송
                            *this.state = SseState::NeedFooter;
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }

                SseState::NeedFooter => {
                    // 푸터 이벤트 전송
                    let took_ms = this.start_time.elapsed().as_millis() as u64;
                    let event = SseEventBuilder::footer_event(*this.event_id, took_ms, None);
                    *this.state = SseState::Done;
                    return Poll::Ready(Some(Ok(event)));
                }

                SseState::Done => {
                    return Poll::Ready(None);
                }
            }
        }
    }
}

/// SSE 스트림 빌더
pub struct SseStreamBuilder {
    columns: Vec<String>,
    query_id: String,
    buffer_size: usize,
    total_rows: Option<u64>,
    heartbeat_interval: Option<Duration>,
}

impl SseStreamBuilder {
    /// 새 빌더 생성
    pub fn new(columns: Vec<String>, query_id: String) -> Self {
        Self {
            columns,
            query_id,
            buffer_size: 1024,
            total_rows: None,
            heartbeat_interval: None,
        }
    }

    /// 버퍼 크기 설정
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// 총 행 수 설정
    pub fn total_rows(mut self, total: u64) -> Self {
        self.total_rows = Some(total);
        self
    }

    /// 하트비트 간격 설정
    pub fn heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = Some(interval);
        self
    }

    /// 스트림과 전송자 생성
    pub fn build(
        self,
    ) -> (
        SseStream,
        mpsc::Sender<Result<serde_json::Value, StreamError>>,
    ) {
        let (tx, rx) = mpsc::channel(self.buffer_size);
        let mut stream = SseStream::new(self.columns, self.query_id, rx);

        if let Some(total) = self.total_rows {
            stream = stream.with_total_rows(total);
        }
        if let Some(interval) = self.heartbeat_interval {
            stream = stream.with_heartbeat(interval);
        }

        (stream, tx)
    }
}
