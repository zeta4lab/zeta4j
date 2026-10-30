//! K-Core Decomposition 알고리즘
//!
//! 그래프의 핵심 구조를 분석합니다.
//!
//! - K-Core: 모든 노드가 최소 k개의 이웃을 가지는 최대 부분 그래프
//! - Core Number: 노드가 속할 수 있는 최대 k 값
//! - K-Shell: k-core에만 속하는 노드 집합
//! - Shell Decomposition: 그래프를 shell로 분해
//! - Degeneracy: 그래프의 퇴화도 (최대 core number)
//! - K-Core Analysis: 통합 분석

mod core;

pub use core::{
    CoreNumberResult,
    // K-Core
    KCore,
    // K-Core Analysis (통합 분석)
    KCoreAnalysisResult,
    KCoreConfig,
    KCoreResult,
    // K-Shell
    KShellResult,
    ShellDecomposition,
    compute_core_numbers,
    decompose_shells,
    degeneracy,
    find_k_core,
    find_k_shell,
    kcore_analyze,
    main_core,
    node_core_number,
};
