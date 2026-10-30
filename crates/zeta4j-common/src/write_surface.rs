//! Persistent surfaces that participate in one atomic database commit.

use thiserror::Error;

/// Persistent surfaces prepared for one atomic commit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteSurfaceSet {
    graph: bool,
    fulltext: bool,
    vector: bool,
}

impl WriteSurfaceSet {
    /// Empty set used while component changes are being prepared.
    pub const fn new() -> Self {
        Self {
            graph: false,
            fulltext: false,
            vector: false,
        }
    }

    pub const fn with_graph(mut self) -> Self {
        self.graph = true;
        self
    }

    pub const fn with_fulltext(mut self) -> Self {
        self.fulltext = true;
        self
    }

    pub const fn with_vector(mut self) -> Self {
        self.vector = true;
        self
    }

    pub const fn has_graph(self) -> bool {
        self.graph
    }

    pub const fn has_fulltext(self) -> bool {
        self.fulltext
    }

    pub const fn has_vector(self) -> bool {
        self.vector
    }

    /// Every commit writes the property graph; fulltext and vector indexes join only when declared.
    pub fn validate(self) -> Result<(), WriteSurfaceError> {
        if self.graph {
            Ok(())
        } else {
            Err(WriteSurfaceError::MissingGraph)
        }
    }
}

/// Invalid set of prepared persistent surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WriteSurfaceError {
    /// The graph surface was not prepared.
    #[error("commit requires the graph surface")]
    MissingGraph,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_is_required_and_semantic_indexes_are_optional() {
        let graph = WriteSurfaceSet::new().with_graph();
        assert!(graph.validate().is_ok());
        assert!(graph.with_fulltext().with_vector().validate().is_ok());
        assert_eq!(
            WriteSurfaceSet::new().with_fulltext().validate(),
            Err(WriteSurfaceError::MissingGraph)
        );
    }
}
