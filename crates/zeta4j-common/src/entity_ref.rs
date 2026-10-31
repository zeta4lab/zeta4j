// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Canonical identity of data indexed by derived semantic search surfaces.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalEntityRef {
    GraphNode { element_id: String },
}

impl CanonicalEntityRef {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::GraphNode { element_id } if element_id.trim().is_empty() => {
                Err("graph element_id must not be empty")
            }
            _ => Ok(()),
        }
    }
}
