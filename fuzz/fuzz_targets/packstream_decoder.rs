// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use libfuzzer_sys::fuzz_target;

const MAX_FUZZ_INPUT_BYTES: usize = 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_FUZZ_INPUT_BYTES {
        return;
    }
    if let Some(encoded) = data.strip_prefix(b"hex:") {
        let Ok(decoded) = hex::decode(encoded) else {
            return;
        };
        let _ = zeta4j_bolt_protocol::packstream::decode(&decoded);
    } else {
        let _ = zeta4j_bolt_protocol::packstream::decode(data);
    }
});
