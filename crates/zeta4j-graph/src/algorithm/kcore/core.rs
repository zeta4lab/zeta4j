// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! K-Core 알고리즘
//!
//! K-Core: 모든 노드가 최소 k개의 이웃을 가지는 최대 부분 그래프
//! Core Number: 노드가 속할 수 있는 최대 k 값
//!
//! Batagelj-Zaversnik 알고리즘 사용 (O(V + E))

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// K-Core 설정
#[derive(Debug, Clone)]
pub struct KCoreConfig {
    /// 최소 degree (k 값)
    pub k: usize,
    /// 관계 타입 필터 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
}

impl Default for KCoreConfig {
    fn default() -> Self {
        Self {
            k: 1,
            relationship_types: None,
        }
    }
}

impl KCoreConfig {
    /// k 값 설정
    pub fn with_k(mut self, k: usize) -> Self {
        self.k = k;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// K-Core 결과
#[derive(Debug, Clone)]
pub struct KCoreResult {
    /// k-core에 속하는 노드 목록
    pub nodes: Vec<NodeId>,
    /// k 값
    pub k: usize,
    /// 노드 수
    pub node_count: usize,
}

/// Core Number 결과
#[derive(Debug, Clone)]
pub struct CoreNumberResult {
    /// 각 노드의 core number
    pub core_numbers: HashMap<NodeId, usize>,
    /// 최대 core number (degeneracy)
    pub max_core: usize,
    /// 노드 수
    pub node_count: usize,
}

/// K-Shell 결과
#[derive(Debug, Clone)]
pub struct KShellResult {
    /// k-shell에 속하는 노드 목록
    pub nodes: Vec<NodeId>,
    /// shell 번호 (= k)
    pub shell: usize,
    /// 노드 수
    pub node_count: usize,
}

/// Shell Decomposition 결과
#[derive(Debug, Clone)]
pub struct ShellDecomposition {
    /// shell 번호 -> 노드 목록
    pub shells: HashMap<usize, Vec<NodeId>>,
    /// 최대 shell 번호
    pub max_shell: usize,
    /// 총 노드 수
    pub node_count: usize,
}

/// K-Core 통합 분석 결과
#[derive(Debug, Clone)]
pub struct KCoreAnalysisResult {
    /// Core number 결과
    pub core_numbers: CoreNumberResult,
    /// Shell 분해 결과
    pub shells: ShellDecomposition,
    /// 그래프 퇴화도
    pub degeneracy: usize,
    /// Main Core (최대 k-core)
    pub main_core: KCoreResult,
    /// 총 노드 수
    pub node_count: usize,
    /// Shell 수
    pub shell_count: usize,
}

/// K-Core 분석기
pub struct KCore<'a> {
    engine: &'a GraphEngine,
    config: KCoreConfig,
}

impl<'a> KCore<'a> {
    /// 새로운 KCore 분석기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: KCoreConfig::default(),
        }
    }

    /// 설정과 함께 분석기 생성
    pub fn with_config(engine: &'a GraphEngine, config: KCoreConfig) -> Self {
        Self { engine, config }
    }

    /// 노드의 이웃 가져오기 (무방향)
    async fn get_neighbors(&self, node_id: NodeId) -> Result<HashSet<NodeId>> {
        let mut neighbors = HashSet::new();
        let edge_ids = self.engine.get_connected_edges(node_id).await;

        for edge_id in edge_ids {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from == node_id {
                    neighbors.insert(to);
                } else if to == node_id {
                    neighbors.insert(from);
                }
            }
        }

        Ok(neighbors)
    }

    /// 모든 노드 ID 가져오기
    async fn get_all_node_ids(&self) -> Result<Vec<NodeId>> {
        let mut nodes = Vec::new();
        let node_count = self.engine.node_count().await?;

        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
            }
        }

        Ok(nodes)
    }

    /// Core Number 계산 (Batagelj-Zaversnik 알고리즘)
    ///
    /// 모든 노드의 core number를 O(V + E) 시간에 계산합니다.
    pub async fn compute_core_numbers(&self) -> Result<CoreNumberResult> {
        let nodes = self.get_all_node_ids().await?;

        if nodes.is_empty() {
            return Ok(CoreNumberResult {
                core_numbers: HashMap::new(),
                max_core: 0,
                node_count: 0,
            });
        }

        // 각 노드의 현재 degree 계산
        let mut degree: HashMap<NodeId, usize> = HashMap::new();
        let mut neighbors_map: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

        for &node in &nodes {
            let node_neighbors = self.get_neighbors(node).await?;
            degree.insert(node, node_neighbors.len());
            neighbors_map.insert(node, node_neighbors);
        }

        // 결과 저장
        let mut core_numbers: HashMap<NodeId, usize> = HashMap::new();
        let mut removed: HashSet<NodeId> = HashSet::new();
        let mut max_core = 0;

        // degree 순으로 정렬된 우선순위 큐 (최소 힙)
        let mut heap: BinaryHeap<Reverse<(usize, NodeId)>> =
            nodes.iter().map(|&n| Reverse((degree[&n], n))).collect();

        while let Some(Reverse((d, node))) = heap.pop() {
            // 이미 처리된 노드 스킵
            if removed.contains(&node) {
                continue;
            }

            // 현재 degree가 다르면 스킵 (이미 업데이트됨)
            if degree.get(&node).copied().unwrap_or(0) != d {
                continue;
            }

            // core number 기록
            let core = d;
            core_numbers.insert(node, core);
            max_core = max_core.max(core);
            removed.insert(node);

            // 이웃들의 degree 감소 (현재 core보다 높은 degree를 가진 이웃만)
            if let Some(neighbors) = neighbors_map.get(&node) {
                for &neighbor in neighbors {
                    if !removed.contains(&neighbor)
                        && let Some(deg) = degree.get_mut(&neighbor)
                    {
                        // Batagelj-Zaversnik: 현재 core보다 높은 degree만 감소
                        if *deg > core {
                            *deg -= 1;
                            // 새로운 degree로 다시 힙에 추가
                            heap.push(Reverse((*deg, neighbor)));
                        }
                    }
                }
            }
        }

        Ok(CoreNumberResult {
            core_numbers,
            max_core,
            node_count: nodes.len(),
        })
    }

    /// 그래프 퇴화도 (Degeneracy) 계산
    ///
    /// 그래프에서 가장 큰 core number를 반환합니다.
    pub async fn degeneracy(&self) -> Result<usize> {
        let result = self.compute_core_numbers().await?;
        Ok(result.max_core)
    }

    /// K-Core 추출
    ///
    /// 모든 노드가 최소 k개의 이웃을 가지는 최대 부분 그래프를 찾습니다.
    pub async fn find_k_core(&self, k: usize) -> Result<KCoreResult> {
        let core_numbers = self.compute_core_numbers().await?;

        // core number >= k인 노드들이 k-core
        let nodes: Vec<NodeId> = core_numbers
            .core_numbers
            .iter()
            .filter(|&(_, &core)| core >= k)
            .map(|(&node, _)| node)
            .collect();

        let mut sorted_nodes = nodes.clone();
        sorted_nodes.sort();

        Ok(KCoreResult {
            node_count: sorted_nodes.len(),
            nodes: sorted_nodes,
            k,
        })
    }

    /// 특정 노드의 Core Number 조회
    pub async fn node_core_number(&self, node_id: NodeId) -> Result<usize> {
        let result = self.compute_core_numbers().await?;
        Ok(result.core_numbers.get(&node_id).copied().unwrap_or(0))
    }

    /// 노드가 k-core에 속하는지 확인
    pub async fn is_in_k_core(&self, node_id: NodeId, k: usize) -> Result<bool> {
        let core = self.node_core_number(node_id).await?;
        Ok(core >= k)
    }

    /// Main Core 추출 (최대 k-core)
    ///
    /// degeneracy와 같은 k 값을 가진 k-core를 반환합니다.
    pub async fn main_core(&self) -> Result<KCoreResult> {
        let core_numbers = self.compute_core_numbers().await?;
        self.find_k_core(core_numbers.max_core).await
    }

    /// K-Shell 추출
    ///
    /// k-core에는 속하지만 (k+1)-core에는 속하지 않는 노드 집합을 찾습니다.
    /// 즉, core number가 정확히 k인 노드들입니다.
    pub async fn find_k_shell(&self, k: usize) -> Result<KShellResult> {
        let core_numbers = self.compute_core_numbers().await?;

        // core number == k인 노드들이 k-shell
        let mut nodes: Vec<NodeId> = core_numbers
            .core_numbers
            .iter()
            .filter(|&(_, &core)| core == k)
            .map(|(&node, _)| node)
            .collect();

        nodes.sort();

        Ok(KShellResult {
            node_count: nodes.len(),
            nodes,
            shell: k,
        })
    }

    /// 노드가 k-shell에 속하는지 확인
    pub async fn is_in_k_shell(&self, node_id: NodeId, k: usize) -> Result<bool> {
        let core = self.node_core_number(node_id).await?;
        Ok(core == k)
    }

    /// 전체 Shell Decomposition
    ///
    /// 그래프를 모든 shell로 분해합니다.
    /// 각 shell은 동일한 core number를 가진 노드들의 집합입니다.
    pub async fn decompose_shells(&self) -> Result<ShellDecomposition> {
        let core_numbers = self.compute_core_numbers().await?;

        let mut shells: HashMap<usize, Vec<NodeId>> = HashMap::new();

        for (&node, &core) in &core_numbers.core_numbers {
            shells.entry(core).or_default().push(node);
        }

        // 각 shell의 노드 목록 정렬
        for nodes in shells.values_mut() {
            nodes.sort();
        }

        Ok(ShellDecomposition {
            max_shell: core_numbers.max_core,
            node_count: core_numbers.node_count,
            shells,
        })
    }

    /// Shell 수 조회
    ///
    /// 그래프에 존재하는 shell의 개수를 반환합니다.
    pub async fn shell_count(&self) -> Result<usize> {
        let decomposition = self.decompose_shells().await?;
        Ok(decomposition.shells.len())
    }

    /// 통합 K-Core 분석
    ///
    /// Core number, Shell 분해, Degeneracy, Main Core 등
    /// 모든 K-Core 관련 분석을 한 번에 수행합니다.
    pub async fn analyze(&self) -> Result<KCoreAnalysisResult> {
        // Core number 계산 (한 번만 수행)
        let core_numbers = self.compute_core_numbers().await?;
        let degeneracy = core_numbers.max_core;
        let node_count = core_numbers.node_count;

        // Shell 분해 (core_numbers 재사용)
        let mut shells_map: HashMap<usize, Vec<NodeId>> = HashMap::new();
        for (&node, &core) in &core_numbers.core_numbers {
            shells_map.entry(core).or_default().push(node);
        }
        for nodes in shells_map.values_mut() {
            nodes.sort();
        }
        let shells = ShellDecomposition {
            max_shell: degeneracy,
            node_count,
            shells: shells_map.clone(),
        };
        let shell_count = shells.shells.len();

        // Main Core 추출
        let main_core_nodes: Vec<NodeId> = core_numbers
            .core_numbers
            .iter()
            .filter(|&(_, &core)| core >= degeneracy)
            .map(|(&node, _)| node)
            .collect();
        let mut sorted_nodes = main_core_nodes;
        sorted_nodes.sort();
        let main_core = KCoreResult {
            node_count: sorted_nodes.len(),
            nodes: sorted_nodes,
            k: degeneracy,
        };

        Ok(KCoreAnalysisResult {
            core_numbers,
            shells,
            degeneracy,
            main_core,
            node_count,
            shell_count,
        })
    }
}

/// Core Number 계산 (편의 함수)
pub async fn compute_core_numbers(engine: &GraphEngine) -> Result<CoreNumberResult> {
    KCore::new(engine).compute_core_numbers().await
}

/// K-Core 추출 (편의 함수)
pub async fn find_k_core(engine: &GraphEngine, k: usize) -> Result<KCoreResult> {
    KCore::new(engine).find_k_core(k).await
}

/// 그래프 퇴화도 계산 (편의 함수)
pub async fn degeneracy(engine: &GraphEngine) -> Result<usize> {
    KCore::new(engine).degeneracy().await
}

/// 노드의 Core Number 조회 (편의 함수)
pub async fn node_core_number(engine: &GraphEngine, node_id: NodeId) -> Result<usize> {
    KCore::new(engine).node_core_number(node_id).await
}

/// Main Core 추출 (편의 함수)
pub async fn main_core(engine: &GraphEngine) -> Result<KCoreResult> {
    KCore::new(engine).main_core().await
}

/// K-Shell 추출 (편의 함수)
pub async fn find_k_shell(engine: &GraphEngine, k: usize) -> Result<KShellResult> {
    KCore::new(engine).find_k_shell(k).await
}

/// 전체 Shell Decomposition (편의 함수)
pub async fn decompose_shells(engine: &GraphEngine) -> Result<ShellDecomposition> {
    KCore::new(engine).decompose_shells().await
}

/// 통합 K-Core 분석 (편의 함수)
pub async fn kcore_analyze(engine: &GraphEngine) -> Result<KCoreAnalysisResult> {
    KCore::new(engine).analyze().await
}
