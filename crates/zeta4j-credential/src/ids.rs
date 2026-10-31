// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::CredentialFailure;

macro_rules! opaque_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new() -> Self {
                Self(format!("{}{}", $prefix, Uuid::now_v7().simple()))
            }

            pub fn parse(value: impl Into<String>) -> Result<Self, CredentialFailure> {
                let value = value.into();
                let suffix = value
                    .strip_prefix($prefix)
                    .ok_or(CredentialFailure::Malformed)?;
                if suffix.len() != 32
                    || !suffix
                        .as_bytes()
                        .iter()
                        .all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(CredentialFailure::Malformed);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = CredentialFailure;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }
    };
}

opaque_id!(CredentialSubjectId, "sub_");
opaque_id!(CredentialId, "cred_");
opaque_id!(SessionId, "sess_");
opaque_id!(TokenFamilyId, "fam_");
opaque_id!(TokenId, "tok_");
opaque_id!(CorrelationId, "corr_");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyVersion(u64);

impl KeyVersion {
    pub fn new(value: u64) -> Result<Self, CredentialFailure> {
        if value == 0 {
            return Err(CredentialFailure::KeyVersion);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn checked_next(self) -> Result<Self, CredentialFailure> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(CredentialFailure::KeyVersion)
    }
}
