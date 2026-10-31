// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Prelude module for convenient imports.
//!
//! This module re-exports the most commonly used types from the crate.
//!
//! # Example
//!
//! ```rust
//! use zeta4j_common::prelude::*;
//!
//! fn example() -> Result<()> {
//!     Ok(())
//! }
//! ```

// =============================================================================
// Error Handling
// =============================================================================

pub use crate::error::{ErrorCode, Result, ZetaError, codes};

// =============================================================================
// Constraint Types
// =============================================================================

pub use crate::constraint_types::{CascadeAction, ReferentialAction};

// =============================================================================
// Common Types
// =============================================================================

pub use crate::types::*;
