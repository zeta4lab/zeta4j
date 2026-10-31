// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 결과 이터레이터 모듈

/// 결과 이터레이터
///
/// 대용량 결과 집합을 효율적으로 순회하기 위한 이터레이터입니다.
/// 내부 데이터 소스를 감싸고 위치 추적 및 배치 가져오기를 지원합니다.
pub struct ResultIterator<T> {
    /// 내부 데이터 (버퍼)
    buffer: Vec<T>,
    /// 현재 버퍼 내 위치
    buffer_position: usize,
    /// 전체 위치 (총 가져온 항목 수)
    total_position: usize,
}

impl<T> ResultIterator<T> {
    /// 빈 이터레이터 생성
    pub fn empty() -> Self {
        Self {
            buffer: Vec::new(),
            buffer_position: 0,
            total_position: 0,
        }
    }

    /// 데이터에서 이터레이터 생성
    pub fn from_vec(data: Vec<T>) -> Self {
        Self {
            buffer: data,
            buffer_position: 0,
            total_position: 0,
        }
    }

    /// 현재 전체 위치
    pub fn position(&self) -> usize {
        self.total_position
    }

    /// 버퍼에 남은 항목 수
    pub fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.buffer_position)
    }

    /// 버퍼가 비어있는지
    pub fn is_exhausted(&self) -> bool {
        self.buffer_position >= self.buffer.len()
    }

    /// N개 스킵
    pub fn skip(&mut self, n: usize) {
        let skip_count = n.min(self.remaining());
        self.buffer_position += skip_count;
        self.total_position += skip_count;
    }

    /// 위치 리셋 (버퍼 시작으로)
    pub fn reset(&mut self) {
        self.buffer_position = 0;
        self.total_position = 0;
    }

    /// 현재 항목 peek
    pub fn peek(&self) -> Option<&T> {
        self.buffer.get(self.buffer_position)
    }

    /// 버퍼에 데이터 추가
    pub fn extend(&mut self, data: impl IntoIterator<Item = T>) {
        self.buffer.extend(data);
    }

    /// 버퍼 교체
    pub fn replace_buffer(&mut self, data: Vec<T>) {
        self.buffer = data;
        self.buffer_position = 0;
    }
}

impl<T: Clone> ResultIterator<T> {
    /// 다음 N개 가져오기 (Clone 필요)
    pub fn take_next(&mut self, n: usize) -> Vec<T> {
        let start = self.buffer_position;
        let end = (start + n).min(self.buffer.len());
        let count = end - start;

        if count == 0 {
            return Vec::new();
        }

        let result: Vec<T> = self.buffer[start..end].to_vec();
        self.buffer_position = end;
        self.total_position += count;

        result
    }

    /// 버퍼 전체를 Vec으로 복사
    pub fn to_vec(&self) -> Vec<T> {
        self.buffer[self.buffer_position..].to_vec()
    }
}

/// 소비형 결과 이터레이터
///
/// 버퍼의 소유권을 가져와 효율적으로 순회합니다.
pub struct IntoResultIterator<T> {
    /// 내부 이터레이터
    inner: std::vec::IntoIter<T>,
    /// 현재 위치
    position: usize,
}

impl<T> IntoResultIterator<T> {
    /// 데이터에서 생성
    pub fn new(data: Vec<T>) -> Self {
        Self {
            inner: data.into_iter(),
            position: 0,
        }
    }

    /// 현재 위치
    pub fn position(&self) -> usize {
        self.position
    }
}

impl<T> Iterator for IntoResultIterator<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next()?;
        self.position += 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for IntoResultIterator<T> {}

/// 청크 이터레이터
///
/// 데이터를 청크 단위로 가져옵니다.
pub struct ChunkIterator<T> {
    /// 데이터 버퍼
    data: Vec<T>,
    /// 청크 크기
    chunk_size: usize,
    /// 현재 위치
    position: usize,
}

impl<T> ChunkIterator<T> {
    /// 새 청크 이터레이터 생성
    pub fn new(data: Vec<T>, chunk_size: usize) -> Self {
        Self {
            data,
            chunk_size: chunk_size.max(1),
            position: 0,
        }
    }

    /// 청크 크기
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// 현재 위치
    pub fn position(&self) -> usize {
        self.position
    }

    /// 남은 항목 수
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.position)
    }

    /// 남은 청크 수
    pub fn remaining_chunks(&self) -> usize {
        let remaining = self.remaining();
        remaining.div_ceil(self.chunk_size)
    }
}

impl<T: Clone> Iterator for ChunkIterator<T> {
    type Item = Vec<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.data.len() {
            return None;
        }

        let start = self.position;
        let end = (start + self.chunk_size).min(self.data.len());
        self.position = end;

        Some(self.data[start..end].to_vec())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining_chunks();
        (remaining, Some(remaining))
    }
}

impl<T: Clone> ExactSizeIterator for ChunkIterator<T> {}
