// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Topological Sort 알고리즘 모듈
//!
//! DAG(Directed Acyclic Graph)의 위상 정렬 및 사이클 탐지를 제공합니다.
//!
//! - Cycle Detection: 사이클 탐지
//! - Kahn's Algorithm: BFS 기반 위상 정렬
//! - DFS Topological Sort: DFS 기반 위상 정렬

mod cycle;
mod dfs_sort;
mod kahn;

pub use cycle::{CycleConfig, CycleDetector, CycleResult, find_cycle, has_cycle, is_dag};

pub use kahn::{
    TopologicalAlgorithm, TopologicalConfig, TopologicalResult, TopologicalSort, dag_sinks,
    dag_sources, topological_sort, topological_sort_with_levels,
};

pub use dfs_sort::{DfsTopologicalSort, dfs_topological_sort};
