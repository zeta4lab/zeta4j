// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Shared wire-level Bolt protocol types.

pub mod message;
pub mod packstream;
mod version;

pub use version::BoltVersion;

/// Bolt protocol magic number (`0x6060B017`).
pub const BOLT_MAGIC: [u8; 4] = [0x60, 0x60, 0xB0, 0x17];
