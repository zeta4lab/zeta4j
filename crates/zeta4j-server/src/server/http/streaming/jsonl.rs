// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! JSON Lines Streaming (M2.4)
//!
//! JSON Lines (ndjson) 포맷 스트리밍

use bytes::Bytes;
use futures::Stream;
use pin_project_lite::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::sync::mpsc;

use super::types::{
    JsonLinesRecord, RecordMeta, StreamError, StreamFooter, StreamHeader, StreamStats,
};

/// JSON Lines 포맷터
pub struct JsonLinesFormatter;

impl JsonLinesFormatter {
    /// 헤더 라인 생성
    pub fn format_header(columns: &[String], query_id: &str, total_rows: Option<u64>) -> Bytes {
        let header = StreamHeader {
            columns: columns.to_vec(),
            total_rows,
            query_id: query_id.to_string(),
        };
        let mut bytes = serde_json::to_vec(&header).unwrap_or_default();
        bytes.push(b'\n');
        Bytes::from(bytes)
    }

    /// 데이터 라인 생성
    pub fn format_record(row: serde_json::Value, index: u64, total: Option<u64>) -> Bytes {
        let record = JsonLinesRecord {
            data: row,
            meta: Some(RecordMeta { index, total }),
        };
        let mut bytes = serde_json::to_vec(&record).unwrap_or_default();
        bytes.push(b'\n');
        Bytes::from(bytes)
    }

    /// 데이터 라인 (메타 없이)
    pub fn format_record_simple(row: serde_json::Value) -> Bytes {
        let record = JsonLinesRecord {
            data: row,
            meta: None,
        };
        let mut bytes = serde_json::to_vec(&record).unwrap_or_default();
        bytes.push(b'\n');
        Bytes::from(bytes)
    }

    /// 푸터 라인 생성
    pub fn format_footer(rows_sent: u64, took_ms: u64, stats: Option<StreamStats>) -> Bytes {
        let footer = StreamFooter {
            rows_sent,
            took_ms,
            stats,
            bookmark: None,
        };
        let mut bytes = serde_json::to_vec(&footer).unwrap_or_default();
        bytes.push(b'\n');
        Bytes::from(bytes)
    }

    /// 에러 라인 생성
    pub fn format_error(error: &StreamError) -> Bytes {
        let error_obj = serde_json::json!({
            "error": error.to_string(),
            "code": error.code()
        });
        let mut bytes = serde_json::to_vec(&error_obj).unwrap_or_default();
        bytes.push(b'\n');
        Bytes::from(bytes)
    }
}

/// JSON Lines 스트림 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamState {
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
    /// JSON Lines 스트림
    pub struct JsonLinesStream {
        columns: Vec<String>,
        query_id: String,
        #[pin]
        receiver: mpsc::Receiver<Result<serde_json::Value, StreamError>>,
        state: StreamState,
        index: u64,
        total_rows: Option<u64>,
        start_time: Instant,
        include_meta: bool,
    }
}

impl JsonLinesStream {
    /// 새 JSON Lines 스트림 생성
    pub fn new(
        columns: Vec<String>,
        query_id: String,
        receiver: mpsc::Receiver<Result<serde_json::Value, StreamError>>,
    ) -> Self {
        Self {
            columns,
            query_id,
            receiver,
            state: StreamState::Initial,
            index: 0,
            total_rows: None,
            start_time: Instant::now(),
            include_meta: true,
        }
    }

    /// 메타데이터 포함 여부 설정
    pub fn with_meta(mut self, include: bool) -> Self {
        self.include_meta = include;
        self
    }

    /// 총 행 수 설정
    pub fn with_total_rows(mut self, total: u64) -> Self {
        self.total_rows = Some(total);
        self
    }
}

impl Stream for JsonLinesStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            match *this.state {
                StreamState::Initial => {
                    // 헤더 전송
                    let header = JsonLinesFormatter::format_header(
                        this.columns,
                        this.query_id,
                        *this.total_rows,
                    );
                    *this.state = StreamState::Streaming;
                    return Poll::Ready(Some(Ok(header)));
                }

                StreamState::Streaming => {
                    // 데이터 수신
                    match this.receiver.as_mut().poll_recv(cx) {
                        Poll::Ready(Some(Ok(row))) => {
                            let bytes = if *this.include_meta {
                                let idx = *this.index;
                                *this.index += 1;
                                JsonLinesFormatter::format_record(row, idx, *this.total_rows)
                            } else {
                                *this.index += 1;
                                JsonLinesFormatter::format_record_simple(row)
                            };
                            return Poll::Ready(Some(Ok(bytes)));
                        }
                        Poll::Ready(Some(Err(error))) => {
                            let bytes = JsonLinesFormatter::format_error(&error);
                            *this.state = StreamState::Done;
                            return Poll::Ready(Some(Ok(bytes)));
                        }
                        Poll::Ready(None) => {
                            // 채널 닫힘 - 푸터 전송
                            *this.state = StreamState::NeedFooter;
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }

                StreamState::NeedFooter => {
                    // 푸터 전송
                    let took_ms = this.start_time.elapsed().as_millis() as u64;
                    let footer = JsonLinesFormatter::format_footer(*this.index, took_ms, None);
                    *this.state = StreamState::Done;
                    return Poll::Ready(Some(Ok(footer)));
                }

                StreamState::Done => {
                    return Poll::Ready(None);
                }
            }
        }
    }
}

/// JSON Lines 스트림 빌더
pub struct JsonLinesStreamBuilder {
    columns: Vec<String>,
    query_id: String,
    buffer_size: usize,
    include_meta: bool,
    total_rows: Option<u64>,
}

impl JsonLinesStreamBuilder {
    /// 새 빌더 생성
    pub fn new(columns: Vec<String>, query_id: String) -> Self {
        Self {
            columns,
            query_id,
            buffer_size: 1024,
            include_meta: true,
            total_rows: None,
        }
    }

    /// 버퍼 크기 설정
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// 메타데이터 포함 여부
    pub fn include_meta(mut self, include: bool) -> Self {
        self.include_meta = include;
        self
    }

    /// 총 행 수 설정
    pub fn total_rows(mut self, total: u64) -> Self {
        self.total_rows = Some(total);
        self
    }

    /// 스트림과 전송자 생성
    pub fn build(
        self,
    ) -> (
        JsonLinesStream,
        mpsc::Sender<Result<serde_json::Value, StreamError>>,
    ) {
        let (tx, rx) = mpsc::channel(self.buffer_size);
        let mut stream =
            JsonLinesStream::new(self.columns, self.query_id, rx).with_meta(self.include_meta);

        if let Some(total) = self.total_rows {
            stream = stream.with_total_rows(total);
        }

        (stream, tx)
    }
}
