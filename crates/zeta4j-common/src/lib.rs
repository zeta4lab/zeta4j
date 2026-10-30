//! # zeta4j-common
//!
//! 모든 zeta4j 크레이트에서 공유하는 공통 타입과 트레이트를 정의합니다.
//!
//! ## Quick Start
//!
//! ```rust
//! use zeta4j_common::prelude::*;
//!
//! fn example() -> Result<()> {
//!     Ok(())
//! }
//! ```
//!
//! ## API Tiers
//!
//! | Tier | Access | Description |
//! |------|--------|-------------|
//! | **prelude** | `use zeta4j_common::prelude::*` | Core types for everyday use |
//! | **root** | `use zeta4j_common::*` | All commonly used types |
//! | **module** | `use zeta4j_common::config::*` | Module-specific types |
//!
//! ## Module Structure
//!
//! - [`defaults`] - Default values for all configurations
//! - [`error`] - Unified error type (`ZetaError`, `Result`)
//! - [`config`] - Common configuration types
//! - [`types`] - Common type definitions
//! - [`traits`] - Common traits
//! - [`constraint_types`] - Database constraint types

// =============================================================================
// Module Structure
// =============================================================================

pub mod bootstrap_envelope;
pub mod config;
pub mod config_templates;
pub mod constraint_types;
pub mod defaults;
pub mod entity_ref;
pub mod error;
pub mod prelude;
pub mod private_fs;
pub mod runtime_profile;
pub mod secret_source;
pub mod state_format;
pub mod traits;
pub mod types;
pub mod write_surface;

// =============================================================================
// Public API - Error Handling
// =============================================================================

pub use bootstrap_envelope::{
    BOOTSTRAP_ENVELOPE_VERSION, DEFAULT_BOOTSTRAP_TTL_SECS, render_bootstrap_envelope,
};
pub use error::{ErrorCode, Result, ZetaError, codes};
pub use runtime_profile::{ParseRuntimeProfileError, RuntimeProfile, is_numeric_loopback};
pub use secret_source::{SecretSourceError, load_secret_file, load_secret_source};

pub use defaults::{
    DEFAULT_DATABASE_NAME, Edition, STATUS_COMPLETED, STATUS_FAILED, STATUS_RUNNING,
    escape_cypher_regex, escape_cypher_string,
};

pub use entity_ref::CanonicalEntityRef;
pub use state_format::{
    CURRENT_CONFIG_FORMAT, CURRENT_STORAGE_FORMAT, GENERATED_CONFIG_FILES, STATE_MANIFEST_FILE,
    StateFormatError, StateFormatStatus, StateManifest, prepare_data_directory,
    prepare_runtime_state, validate_data_directory,
};
pub use write_surface::{WriteSurfaceError, WriteSurfaceSet};

// =============================================================================
// Public API - Constraint Types
// =============================================================================

pub use config_templates::{DEFAULT_CORE_TOML, DEFAULT_SERVER_TOML};
pub use constraint_types::{CascadeAction, ReferentialAction};
