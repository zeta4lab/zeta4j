//! Maximum Flow 알고리즘 모듈
//!
//! 네트워크 최대 유량 및 최소 컷 알고리즘을 제공합니다.
//!
//! - Edmonds-Karp: BFS 기반 Ford-Fulkerson (O(VE²))
//! - Push-Relabel: Preflow-Push 알고리즘 (O(V²E))
//! - Min-Cut: 최대 유량 = 최소 컷 정리 활용

mod edmonds_karp;
mod min_cut;
mod network;
mod push_relabel;

pub use network::{FlowEdge, FlowNetwork};

pub use edmonds_karp::{
    EdmondsKarp, MaxFlowAlgorithm, MaxFlowConfig, MaxFlowResult, max_flow, max_flow_with_capacity,
};

pub use push_relabel::{PushRelabel, max_flow_pr, max_flow_pr_with_capacity};

pub use min_cut::{MinCut, MinCutResult, min_cut, min_cut_with_capacity};
