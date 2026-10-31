// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 경로 탐색 알고리즘
//!
//! - BFS 최단 경로 (무가중치)
//! - Dijkstra 가중치 최단 경로 (양수 가중치)
//! - Bellman-Ford (음수 가중치, 음수 사이클 감지)
//! - A* 휴리스틱 최단 경로
//! - Floyd-Warshall 모든 쌍 최단 경로

mod astar;
mod bellman_ford;
mod bfs;
mod dijkstra;
mod floyd_warshall;

pub use bfs::{BfsShortestPath, ShortestPathResult};
pub use dijkstra::{Dijkstra, DijkstraConfig, DijkstraResult};

pub use bellman_ford::{
    BellmanFord, BellmanFordConfig, BellmanFordResult, bellman_ford, bellman_ford_weighted,
    detect_negative_cycle, detect_negative_cycle_weighted,
};

pub use astar::{
    AStar, AStarConfig, AStarResult, Heuristic, astar, astar_euclidean, astar_manhattan,
    astar_weighted,
};

pub use floyd_warshall::{
    FloydWarshall, FloydWarshallConfig, FloydWarshallResult, all_pairs_distances,
    all_pairs_distances_weighted, floyd_warshall, floyd_warshall_weighted,
};
