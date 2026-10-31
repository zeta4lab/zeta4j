// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 패턴 관련 AST
//!
//! 노드 패턴, 엣지 패턴, 경로 패턴

mod edge;
mod node;

pub use edge::{EdgeDirection, EdgePattern, VariableLengthRange};
pub use node::NodePattern;

/// 경로 함수 (shortestPath, allShortestPaths)
#[derive(Debug, Clone, PartialEq)]
pub enum PathFunction {
    /// shortestPath - BFS 최단 경로 (단일 결과)
    ShortestPath,
    /// allShortestPaths - 모든 최단 경로
    AllShortestPaths,
}

/// 패턴 (노드와 엣지의 경로)
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    /// 패턴 경로 요소들 (노드-엣지-노드-엣지-...)
    pub elements: Vec<PatternElement>,
    /// 하위 호환성을 위한 노드 파트들 (단일 노드 패턴용)
    pub parts: Vec<NodePattern>,
    /// 경로 변수명 (path = ... 형태)
    pub variable: Option<String>,
    /// 경로 함수 (shortestPath, allShortestPaths)
    pub path_function: Option<PathFunction>,
}

/// 패턴 요소 (노드 또는 엣지)
#[derive(Debug, Clone, PartialEq)]
pub enum PatternElement {
    Node(NodePattern),
    Edge(EdgePattern),
}

/// 하위 호환성을 위한 PatternPart 별칭
pub type PatternPart = NodePattern;
