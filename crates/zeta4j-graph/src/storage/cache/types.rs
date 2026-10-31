// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 버퍼 풀 타입 정의

/// 프레임 ID (버퍼 풀 내의 인덱스)
pub type FrameId = usize;

/// 버퍼 풀 통계 정보
#[derive(Debug, Clone)]
pub struct BufferPoolStats {
    pub pool_size: usize,
    pub used_frames: usize,
    pub free_frames: usize,
    pub pinned_frames: usize,
    pub evictable_frames: usize,
}
