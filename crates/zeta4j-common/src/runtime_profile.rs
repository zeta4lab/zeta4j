// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Runtime security profile shared by every product entrypoint.

use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Closed set of runtime security profiles.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeProfile {
    /// Fail-closed profile for every normal deployment.
    #[default]
    Production,
    /// Explicit profile for loopback-only development and tests.
    LocalTest,
}

impl RuntimeProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::LocalTest => "local-test",
        }
    }
}

impl fmt::Display for RuntimeProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Runtime profile parse failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("[SV-BOOT-010] unsupported runtime profile `{value}`; expected production or local-test")]
pub struct ParseRuntimeProfileError {
    value: String,
}

impl FromStr for RuntimeProfile {
    type Err = ParseRuntimeProfileError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "production" => Ok(Self::Production),
            "local-test" => Ok(Self::LocalTest),
            _ => Err(ParseRuntimeProfileError {
                value: value.to_owned(),
            }),
        }
    }
}

/// Returns true only for a numeric loopback IP literal.
///
/// Hostnames are deliberately rejected so DNS changes cannot widen an insecure
/// listener after validation.
pub fn is_numeric_loopback(host: &str) -> bool {
    host.trim_matches(['[', ']'])
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_is_the_default() {
        assert_eq!(RuntimeProfile::default(), RuntimeProfile::Production);
    }

    #[test]
    fn parsing_is_closed_and_case_sensitive() {
        assert_eq!(
            "local-test".parse::<RuntimeProfile>(),
            Ok(RuntimeProfile::LocalTest)
        );
        assert!("local".parse::<RuntimeProfile>().is_err());
        assert!(
            "local"
                .parse::<RuntimeProfile>()
                .err()
                .is_some_and(|error| error.to_string().contains("[SV-BOOT-010]"))
        );
        assert!("Production".parse::<RuntimeProfile>().is_err());
    }

    #[test]
    fn only_numeric_loopback_literals_are_accepted() {
        for allowed in ["127.0.0.1", "127.8.9.10", "::1", "[::1]"] {
            assert!(is_numeric_loopback(allowed), "{allowed}");
        }
        for rejected in ["localhost", "0.0.0.0", "::", "10.0.0.1", "pod.example"] {
            assert!(!is_numeric_loopback(rejected), "{rejected}");
        }
    }
}
