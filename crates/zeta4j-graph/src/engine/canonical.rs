// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Canonical committed graph storage boundary.

use std::collections::{BTreeMap, HashMap};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::core::{Edge, Node};
use crate::error::{GraphError, Result};

use super::GraphTransactionDelta;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphIndexKind {
    Property,
    Composite,
    Spatial,
    Function { function: String },
    Fulltext { analyzer: String },
    Vector { dimensions: u32, distance: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphIndexDefinition {
    pub name: String,
    pub label: String,
    pub properties: Vec<String>,
    pub kind: GraphIndexKind,
}

impl GraphIndexDefinition {
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() || self.label.is_empty() || self.properties.is_empty() {
            return Err(GraphError::Index {
                message: "index name, label, and properties must not be empty".to_string(),
            });
        }
        if self.properties.iter().any(String::is_empty) {
            return Err(GraphError::Index {
                message: "index property names must not be empty".to_string(),
            });
        }
        match &self.kind {
            GraphIndexKind::Property | GraphIndexKind::Spatial if self.properties.len() != 1 => {
                Err(GraphError::Index {
                    message: "property and spatial indexes require exactly one property"
                        .to_string(),
                })
            }
            GraphIndexKind::Function { function }
                if self.properties.len() != 1 || function.is_empty() =>
            {
                Err(GraphError::Index {
                    message: "function indexes require one property and a function".to_string(),
                })
            }
            GraphIndexKind::Fulltext { analyzer }
                if !matches!(analyzer.as_str(), "standard" | "korean" | "english") =>
            {
                Err(GraphError::Index {
                    message: format!("unsupported fulltext analyzer '{analyzer}'"),
                })
            }
            GraphIndexKind::Vector {
                dimensions,
                distance,
            } if self.properties.len() != 1
                || *dimensions == 0
                || !matches!(distance.as_str(), "cosine" | "euclidean" | "dot_product") =>
            {
                Err(GraphError::Index {
                    message: "vector indexes require one property, positive dimensions, and distance cosine, euclidean, or dot_product".to_string(),
                })
            }
            _ => Ok(()),
        }
    }

    pub fn spatial_name(label: &str, property: &str) -> String {
        format!("__spatial__{}:{label}:{property}", label.len())
    }
}

/// Storage-neutral committed graph used by GraphEngine.
pub trait CanonicalGraphStore: Send + Sync {
    fn node(&self, node_id: u64) -> Result<Option<Node>>;
    fn edge(&self, edge_id: u64) -> Result<Option<Edge>>;
    fn all_node_ids(&self) -> Result<Vec<u64>>;
    fn all_edge_ids(&self) -> Result<Vec<u64>>;
    fn nodes_by_label(&self, label: &str) -> Result<Vec<u64>>;
    fn connected_edges(&self, node_id: u64) -> Result<Vec<u64>>;
    fn index_definitions(&self) -> Result<Vec<GraphIndexDefinition>>;
    fn create_index_definition(&self, definition: GraphIndexDefinition) -> Result<u64>;
    fn drop_index_definition(&self, name: &str) -> Result<u64>;
    fn commit_graph(&self, delta: GraphTransactionDelta) -> Result<u64>;
}

/// Canonical in-memory backend for embedded and isolated GraphEngine use.
#[derive(Debug, Default)]
pub struct InMemoryCanonicalGraphStore {
    state: RwLock<InMemoryGraphState>,
}

#[derive(Debug, Default)]
struct InMemoryGraphState {
    nodes: HashMap<u64, Node>,
    edges: HashMap<u64, Edge>,
    index_definitions: BTreeMap<String, GraphIndexDefinition>,
    revision: u64,
}

impl CanonicalGraphStore for InMemoryCanonicalGraphStore {
    fn node(&self, node_id: u64) -> Result<Option<Node>> {
        Ok(self.state.read().nodes.get(&node_id).cloned())
    }

    fn edge(&self, edge_id: u64) -> Result<Option<Edge>> {
        Ok(self.state.read().edges.get(&edge_id).cloned())
    }

    fn all_node_ids(&self) -> Result<Vec<u64>> {
        let mut ids: Vec<_> = self.state.read().nodes.keys().copied().collect();
        ids.sort_unstable();
        Ok(ids)
    }

    fn all_edge_ids(&self) -> Result<Vec<u64>> {
        let mut ids: Vec<_> = self.state.read().edges.keys().copied().collect();
        ids.sort_unstable();
        Ok(ids)
    }

    fn nodes_by_label(&self, label: &str) -> Result<Vec<u64>> {
        let mut ids: Vec<_> = self
            .state
            .read()
            .nodes
            .values()
            .filter(|node| node.labels().contains(label))
            .map(Node::id)
            .collect();
        ids.sort_unstable();
        Ok(ids)
    }

    fn connected_edges(&self, node_id: u64) -> Result<Vec<u64>> {
        let mut ids: Vec<_> = self
            .state
            .read()
            .edges
            .values()
            .filter(|edge| edge.connects_to(node_id))
            .map(Edge::id)
            .collect();
        ids.sort_unstable();
        Ok(ids)
    }

    fn index_definitions(&self) -> Result<Vec<GraphIndexDefinition>> {
        Ok(self
            .state
            .read()
            .index_definitions
            .values()
            .cloned()
            .collect())
    }

    fn create_index_definition(&self, definition: GraphIndexDefinition) -> Result<u64> {
        definition.validate()?;
        let mut state = self.state.write();
        if state.index_definitions.contains_key(&definition.name) {
            return Err(GraphError::Index {
                message: format!("Index '{}' already exists", definition.name),
            });
        }
        state
            .index_definitions
            .insert(definition.name.clone(), definition);
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| GraphError::System {
                message: "canonical graph revision overflow".to_string(),
            })?;
        Ok(state.revision)
    }

    fn drop_index_definition(&self, name: &str) -> Result<u64> {
        let mut state = self.state.write();
        if state.index_definitions.remove(name).is_none() {
            return Err(GraphError::Index {
                message: format!("Index '{name}' not found"),
            });
        }
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| GraphError::System {
                message: "canonical graph revision overflow".to_string(),
            })?;
        Ok(state.revision)
    }

    fn commit_graph(&self, delta: GraphTransactionDelta) -> Result<u64> {
        let mut state = self.state.write();
        let mut nodes = state.nodes.clone();
        let mut edges = state.edges.clone();

        for edge_id in delta.delete_edge_ids {
            edges.remove(&edge_id);
        }
        for node_id in delta.delete_node_ids {
            nodes.remove(&node_id);
        }
        for node in delta.upsert_nodes {
            nodes.insert(node.id(), node);
        }
        for edge in delta.upsert_edges {
            if !nodes.contains_key(&edge.from_node()) || !nodes.contains_key(&edge.to_node()) {
                return Err(GraphError::Constraint {
                    constraint_name: "referential_integrity".to_string(),
                    message: format!(
                        "edge {} references a node absent from the canonical graph",
                        edge.id()
                    ),
                });
            }
            edges.insert(edge.id(), edge);
        }
        if let Some(edge) = edges.values().find(|edge| {
            !nodes.contains_key(&edge.from_node()) || !nodes.contains_key(&edge.to_node())
        }) {
            return Err(GraphError::Constraint {
                constraint_name: "referential_integrity".to_string(),
                message: format!("node deletion leaves dangling edge {}", edge.id()),
            });
        }

        state.nodes = nodes;
        state.edges = edges;
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| GraphError::System {
                message: "canonical graph revision overflow".to_string(),
            })?;
        Ok(state.revision)
    }
}
