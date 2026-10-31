// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 메모리 관련 공통 트레이트
//!
//! 메모리 사용량을 보고하는 타입들을 위한 공통 인터페이스입니다.

/// 메모리 사용량 보고 트레이트
///
/// Stats 구조체들이 구현하여 일관된 메모리 사용량 조회 인터페이스를 제공합니다.
///
/// # 예시
///
/// ```rust,ignore
/// use zeta4j_graph::core::MemoryReporter;
///
/// fn total_memory<T: MemoryReporter>(reporters: &[T]) -> usize {
///     reporters.iter().map(|r| r.memory_usage()).sum()
/// }
/// ```
pub trait MemoryReporter {
    /// 메모리 사용량 반환 (바이트)
    fn memory_usage(&self) -> usize;
}

/// 메모리 크기 계산 트레이트
///
/// 개별 요소의 메모리 크기를 계산하는 타입들을 위한 인터페이스입니다.
/// Node, Edge, Property 등이 구현합니다.
pub trait MemorySized {
    /// 이 요소의 메모리 크기 (바이트)
    fn memory_size(&self) -> usize;
}
