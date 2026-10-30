//! Hamiltonian Path/Circuit 알고리즘
//!
//! 모든 노드를 정확히 한 번씩 방문하는 경로/회로를 찾습니다.
//!
//! # NP-Complete
//!
//! Hamiltonian 문제는 NP-Complete 문제입니다. 다항 시간 알고리즘이 알려져 있지 않습니다.
//!
//! # 알고리즘
//!
//! - **백트래킹**: O(n!) 최악, 가지치기로 개선
//! - **Held-Karp DP**: O(n² * 2^n), 최소 비용 경로 찾기
//!
//! # Hamiltonian vs Eulerian
//!
//! | 구분 | Hamiltonian | Eulerian |
//! |------|-------------|----------|
//! | 방문 대상 | 모든 노드 | 모든 엣지 |
//! | 복잡도 | NP-Complete | O(E) |
//!
//! # 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::hamiltonian::{hamiltonian_path, has_hamiltonian_circuit};
//!
//! // Hamiltonian Path 존재 여부
//! let has_path = has_hamiltonian_path(&engine)?;
//!
//! // Hamiltonian Path 찾기
//! if let Some(result) = hamiltonian_path(&engine)? {
//!     println!("Path: {:?}", result.path);
//! }
//! ```

mod backtrack;
mod held_karp;

pub use backtrack::{
    HamiltonianBacktrack, hamiltonian_circuit, hamiltonian_path, has_hamiltonian_circuit,
    has_hamiltonian_path,
};

pub use held_karp::{
    HeldKarp, WeightedHamiltonianResult, min_hamiltonian_circuit, min_hamiltonian_path,
};

/// Hamiltonian 알고리즘 설정
#[derive(Debug, Clone)]
pub struct HamiltonianConfig {
    /// 최대 노드 수 제한 (기본: 20)
    pub max_nodes: usize,
    /// 타임아웃 (밀리초, 기본: 5000)
    pub timeout_ms: u64,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
    /// 가중치 속성명 (Held-Karp용)
    pub weight_property: Option<String>,
}

impl Default for HamiltonianConfig {
    fn default() -> Self {
        Self {
            max_nodes: 20,
            timeout_ms: 5000,
            relationship_types: None,
            weight_property: None,
        }
    }
}

impl HamiltonianConfig {
    /// 최대 노드 수 설정
    pub fn with_max_nodes(mut self, max: usize) -> Self {
        self.max_nodes = max;
        self
    }

    /// 타임아웃 설정 (밀리초)
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 가중치 속성 설정
    pub fn with_weight_property(mut self, property: impl Into<String>) -> Self {
        self.weight_property = Some(property.into());
        self
    }
}

/// Hamiltonian 결과
#[derive(Debug, Clone)]
pub struct HamiltonianResult {
    /// 경로 (노드 순서)
    pub path: Vec<crate::core::NodeId>,
    /// 회로 여부
    pub is_circuit: bool,
    /// 발견 여부
    pub found: bool,
    /// 탐색한 노드 수
    pub nodes_explored: usize,
}

impl HamiltonianResult {
    /// 빈 결과 (Path)
    pub fn empty() -> Self {
        Self {
            path: Vec::new(),
            is_circuit: false,
            found: true,
            nodes_explored: 0,
        }
    }

    /// 빈 결과 (Circuit)
    pub fn empty_circuit() -> Self {
        Self {
            path: Vec::new(),
            is_circuit: true,
            found: true,
            nodes_explored: 0,
        }
    }
}
