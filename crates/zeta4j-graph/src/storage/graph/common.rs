//! 그래프 스토리지 공통 타입
//!
//! NodeStorage와 EdgeStorage에서 공유하는 통계 타입을 정의합니다.

use crate::core::MemoryReporter;

/// 공용 스토리지 통계 (외부 공개용)
///
/// NodeStorage와 EdgeStorage 모두 동일한 통계 구조를 가집니다.
#[derive(Debug, Clone, Default)]
pub struct ElementStorageStats {
    /// 총 요소 수 (노드 또는 엣지)
    pub total_elements: usize,

    /// 총 메모리 사용량 (바이트)
    pub memory_usage: usize,

    /// 삽입 횟수
    pub insert_count: u64,

    /// 조회 횟수
    pub lookup_count: u64,

    /// 업데이트 횟수
    pub update_count: u64,

    /// 삭제 횟수
    pub delete_count: u64,
}

/// 내부 스토리지 통계 (crate 내부용)
///
/// NodeStorage와 EdgeStorage 내부에서 사용하는 통계 구조체입니다.
#[derive(Debug, Default)]
pub(crate) struct InternalElementStats {
    /// 총 요소 수
    pub total_elements: usize,

    /// 총 메모리 사용량 (바이트)
    pub memory_usage: usize,

    /// 삽입 횟수
    pub insert_count: u64,

    /// 조회 횟수
    pub lookup_count: u64,

    /// 업데이트 횟수
    pub update_count: u64,

    /// 삭제 횟수
    pub delete_count: u64,
}

impl InternalElementStats {
    /// 외부 공개용 통계로 변환
    #[allow(dead_code)]
    pub fn to_public(&self) -> ElementStorageStats {
        ElementStorageStats {
            total_elements: self.total_elements,
            memory_usage: self.memory_usage,
            insert_count: self.insert_count,
            lookup_count: self.lookup_count,
            update_count: self.update_count,
            delete_count: self.delete_count,
        }
    }
}

// MemoryReporter 트레이트 구현
impl MemoryReporter for ElementStorageStats {
    fn memory_usage(&self) -> usize {
        self.memory_usage
    }
}

impl MemoryReporter for InternalElementStats {
    fn memory_usage(&self) -> usize {
        self.memory_usage
    }
}
