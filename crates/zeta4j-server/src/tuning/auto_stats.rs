//! 통계 자동 갱신 모듈

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

/// 통계 자동 갱신 설정
#[derive(Debug, Clone)]
pub struct AutoStatsConfig {
    /// 갱신 주기 (변경 횟수 기준)
    pub update_threshold: usize,
    /// 백그라운드 갱신 활성화
    pub background_refresh: bool,
    /// 샘플링 비율 (0.0 ~ 1.0)
    pub sample_ratio: f64,
}

impl Default for AutoStatsConfig {
    fn default() -> Self {
        Self {
            update_threshold: 1000,
            background_refresh: true,
            sample_ratio: 0.1,
        }
    }
}

/// 변경 유형
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeType {
    /// 노드 삽입 (라벨)
    NodeInsert(String),
    /// 노드 삭제 (라벨)
    NodeDelete(String),
    /// 엣지 삽입 (관계 타입)
    EdgeInsert(String),
    /// 엣지 삭제 (관계 타입)
    EdgeDelete(String),
    /// 속성 업데이트 (라벨 또는 관계 타입)
    PropertyUpdate(String),
}

impl ChangeType {
    /// 관련 라벨/타입 이름 반환
    pub fn name(&self) -> &str {
        match self {
            ChangeType::NodeInsert(name) => name,
            ChangeType::NodeDelete(name) => name,
            ChangeType::EdgeInsert(name) => name,
            ChangeType::EdgeDelete(name) => name,
            ChangeType::PropertyUpdate(name) => name,
        }
    }

    /// 노드 관련 변경인지 확인
    pub fn is_node_change(&self) -> bool {
        matches!(self, ChangeType::NodeInsert(_) | ChangeType::NodeDelete(_))
    }

    /// 엣지 관련 변경인지 확인
    pub fn is_edge_change(&self) -> bool {
        matches!(self, ChangeType::EdgeInsert(_) | ChangeType::EdgeDelete(_))
    }
}

/// 통계 갱신 결과
#[derive(Debug, Clone)]
pub struct RefreshResult {
    /// 갱신된 라벨 수
    pub labels_refreshed: usize,
    /// 갱신된 관계 타입 수
    pub rel_types_refreshed: usize,
    /// 소요 시간 (밀리초)
    pub duration_ms: u64,
    /// 갱신 성공 여부
    pub success: bool,
}

/// 라벨별 변경 카운터
#[derive(Debug, Default)]
struct LabelChangeCounter {
    insert_count: AtomicUsize,
    delete_count: AtomicUsize,
    update_count: AtomicUsize,
}

impl LabelChangeCounter {
    fn total(&self) -> usize {
        self.insert_count.load(Ordering::Relaxed)
            + self.delete_count.load(Ordering::Relaxed)
            + self.update_count.load(Ordering::Relaxed)
    }

    fn reset(&self) {
        self.insert_count.store(0, Ordering::Relaxed);
        self.delete_count.store(0, Ordering::Relaxed);
        self.update_count.store(0, Ordering::Relaxed);
    }
}

/// 통계 자동 갱신기
pub struct AutoStatsRefresher {
    config: AutoStatsConfig,
    /// 전체 변경 카운터
    total_changes: AtomicUsize,
    /// 라벨별 변경 카운터
    label_changes: RwLock<HashMap<String, Arc<LabelChangeCounter>>>,
    /// 관계 타입별 변경 카운터
    rel_type_changes: RwLock<HashMap<String, Arc<LabelChangeCounter>>>,
    /// 마지막 갱신 이후 변경된 라벨들
    dirty_labels: RwLock<Vec<String>>,
    /// 마지막 갱신 이후 변경된 관계 타입들
    dirty_rel_types: RwLock<Vec<String>>,
}

impl AutoStatsRefresher {
    /// 새 AutoStatsRefresher 생성
    pub fn new(config: AutoStatsConfig) -> Self {
        Self {
            config,
            total_changes: AtomicUsize::new(0),
            label_changes: RwLock::new(HashMap::new()),
            rel_type_changes: RwLock::new(HashMap::new()),
            dirty_labels: RwLock::new(Vec::new()),
            dirty_rel_types: RwLock::new(Vec::new()),
        }
    }

    /// 설정 반환
    pub fn config(&self) -> &AutoStatsConfig {
        &self.config
    }

    /// 변경 사항 기록
    pub fn record_change(&self, change_type: ChangeType) {
        self.total_changes.fetch_add(1, Ordering::Relaxed);

        let name = change_type.name().to_string();

        if change_type.is_node_change() || matches!(change_type, ChangeType::PropertyUpdate(_)) {
            let mut label_changes = self
                .label_changes
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let counter = label_changes
                .entry(name.clone())
                .or_insert_with(|| Arc::new(LabelChangeCounter::default()));

            match &change_type {
                ChangeType::NodeInsert(_) => {
                    counter.insert_count.fetch_add(1, Ordering::Relaxed);
                }
                ChangeType::NodeDelete(_) => {
                    counter.delete_count.fetch_add(1, Ordering::Relaxed);
                }
                ChangeType::PropertyUpdate(_) => {
                    counter.update_count.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }

            // dirty 라벨 추가
            let mut dirty = self.dirty_labels.write().unwrap_or_else(|e| e.into_inner());
            if !dirty.contains(&name) {
                dirty.push(name.clone());
            }
        }

        if change_type.is_edge_change() {
            let name = change_type.name().to_string();
            let mut rel_changes = self
                .rel_type_changes
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let counter = rel_changes
                .entry(name.clone())
                .or_insert_with(|| Arc::new(LabelChangeCounter::default()));

            match &change_type {
                ChangeType::EdgeInsert(_) => {
                    counter.insert_count.fetch_add(1, Ordering::Relaxed);
                }
                ChangeType::EdgeDelete(_) => {
                    counter.delete_count.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }

            // dirty 관계 타입 추가
            let mut dirty = self
                .dirty_rel_types
                .write()
                .unwrap_or_else(|e| e.into_inner());
            if !dirty.contains(&name) {
                dirty.push(name);
            }
        }
    }

    /// 전체 변경 횟수 반환
    pub fn total_changes(&self) -> usize {
        self.total_changes.load(Ordering::Relaxed)
    }

    /// 갱신 필요 여부 확인
    pub fn needs_refresh(&self) -> bool {
        self.total_changes.load(Ordering::Relaxed) >= self.config.update_threshold
    }

    /// 특정 라벨의 갱신 필요 여부 확인
    pub fn needs_refresh_label(&self, label: &str) -> bool {
        let label_changes = self.label_changes.read().unwrap_or_else(|e| e.into_inner());
        if let Some(counter) = label_changes.get(label) {
            counter.total() >= self.config.update_threshold / 10
        } else {
            false
        }
    }

    /// 변경된 라벨 목록 반환
    pub fn dirty_labels(&self) -> Vec<String> {
        self.dirty_labels
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 변경된 관계 타입 목록 반환
    pub fn dirty_rel_types(&self) -> Vec<String> {
        self.dirty_rel_types
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 통계 갱신 실행 (실제 통계 수집은 외부에서 수행)
    pub fn refresh(&self) -> RefreshResult {
        let start = std::time::Instant::now();

        let labels_refreshed = {
            let dirty = self.dirty_labels.read().unwrap_or_else(|e| e.into_inner());
            dirty.len()
        };

        let rel_types_refreshed = {
            let dirty = self
                .dirty_rel_types
                .read()
                .unwrap_or_else(|e| e.into_inner());
            dirty.len()
        };

        // 카운터 리셋
        self.reset_counters();

        let duration_ms = start.elapsed().as_millis() as u64;

        RefreshResult {
            labels_refreshed,
            rel_types_refreshed,
            duration_ms,
            success: true,
        }
    }

    /// 카운터 리셋
    fn reset_counters(&self) {
        self.total_changes.store(0, Ordering::Relaxed);

        // 라벨 카운터 리셋
        {
            let label_changes = self.label_changes.read().unwrap_or_else(|e| e.into_inner());
            for counter in label_changes.values() {
                counter.reset();
            }
        }

        // 관계 타입 카운터 리셋
        {
            let rel_changes = self
                .rel_type_changes
                .read()
                .unwrap_or_else(|e| e.into_inner());
            for counter in rel_changes.values() {
                counter.reset();
            }
        }

        // dirty 목록 클리어
        self.dirty_labels
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.dirty_rel_types
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// 라벨별 변경 통계 반환
    pub fn get_label_stats(&self, label: &str) -> Option<(usize, usize, usize)> {
        let label_changes = self.label_changes.read().unwrap_or_else(|e| e.into_inner());
        label_changes.get(label).map(|counter| {
            (
                counter.insert_count.load(Ordering::Relaxed),
                counter.delete_count.load(Ordering::Relaxed),
                counter.update_count.load(Ordering::Relaxed),
            )
        })
    }
}
