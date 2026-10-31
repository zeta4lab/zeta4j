// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! PackStream decoder shared by server and clients.

use bytes::Buf;
use std::collections::HashMap;

use super::PackStreamError;
use super::marker::*;
use super::types::{PackStreamStructure, PackStreamValue};

const MAX_NESTING_DEPTH: usize = 64;

/// PackStream decoder that reads values from a byte buffer.
pub struct PackStreamDecoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> PackStreamDecoder<'a> {
    /// Create a new decoder for the given bytes.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Get the current position.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Get remaining bytes count.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Check if all data has been consumed.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Decode the next value.
    pub fn decode(&mut self) -> Result<PackStreamValue, PackStreamError> {
        self.decode_at_depth(0)
    }

    fn decode_at_depth(&mut self, depth: usize) -> Result<PackStreamValue, PackStreamError> {
        if depth > MAX_NESTING_DEPTH {
            return Err(PackStreamError::NestingTooDeep {
                max: MAX_NESTING_DEPTH,
            });
        }
        let marker = self.read_u8()?;

        // Check for tiny types first (most common)
        if is_tiny_int(marker) && !self.is_special_marker(marker) {
            return Ok(PackStreamValue::Integer(decode_tiny_int(marker) as i64));
        }

        if is_tiny_string(marker) {
            let len = tiny_string_len(marker);
            return self.read_string_data(len);
        }

        if is_tiny_list(marker) {
            let len = tiny_list_len(marker);
            return self.read_list_data(len, depth);
        }

        if is_tiny_map(marker) {
            let len = tiny_map_len(marker);
            return self.read_map_data(len, depth);
        }

        if is_tiny_struct(marker) {
            let len = tiny_struct_fields(marker);
            return self.read_struct_data(len, depth);
        }

        // Handle other markers
        match marker {
            NULL => Ok(PackStreamValue::Null),
            TRUE => Ok(PackStreamValue::Boolean(true)),
            FALSE => Ok(PackStreamValue::Boolean(false)),

            FLOAT_64 => Ok(PackStreamValue::Float(self.read_f64()?)),

            INT_8 => Ok(PackStreamValue::Integer(self.read_i8()? as i64)),
            INT_16 => Ok(PackStreamValue::Integer(self.read_i16()? as i64)),
            INT_32 => Ok(PackStreamValue::Integer(self.read_i32()? as i64)),
            INT_64 => Ok(PackStreamValue::Integer(self.read_i64()?)),

            BYTES_8 => {
                let len = self.read_u8()? as usize;
                self.read_bytes_data(len)
            }
            BYTES_16 => {
                let len = self.read_u16()? as usize;
                self.read_bytes_data(len)
            }
            BYTES_32 => {
                let len = self.read_u32()? as usize;
                self.read_bytes_data(len)
            }

            STRING_8 => {
                let len = self.read_u8()? as usize;
                self.read_string_data(len)
            }
            STRING_16 => {
                let len = self.read_u16()? as usize;
                self.read_string_data(len)
            }
            STRING_32 => {
                let len = self.read_u32()? as usize;
                self.read_string_data(len)
            }

            LIST_8 => {
                let len = self.read_u8()? as usize;
                self.read_list_data(len, depth)
            }
            LIST_16 => {
                let len = self.read_u16()? as usize;
                self.read_list_data(len, depth)
            }
            LIST_32 => {
                let len = self.read_u32()? as usize;
                self.read_list_data(len, depth)
            }

            MAP_8 => {
                let len = self.read_u8()? as usize;
                self.read_map_data(len, depth)
            }
            MAP_16 => {
                let len = self.read_u16()? as usize;
                self.read_map_data(len, depth)
            }
            MAP_32 => {
                let len = self.read_u32()? as usize;
                self.read_map_data(len, depth)
            }

            STRUCT_8 => {
                let len = self.read_u8()? as usize;
                self.read_struct_data(len, depth)
            }
            STRUCT_16 => {
                let len = self.read_u16()? as usize;
                self.read_struct_data(len, depth)
            }

            _ => Err(PackStreamError::UnknownMarker(marker)),
        }
    }

    /// Check if a marker is a special non-tiny-int marker in the tiny int range.
    fn is_special_marker(&self, marker: u8) -> bool {
        matches!(
            marker,
            NULL | TRUE
                | FALSE
                | FLOAT_64
                | INT_8
                | INT_16
                | INT_32
                | INT_64
                | BYTES_8
                | BYTES_16
                | BYTES_32
                | STRING_8
                | STRING_16
                | STRING_32
                | LIST_8
                | LIST_16
                | LIST_32
                | MAP_8
                | MAP_16
                | MAP_32
                | STRUCT_8
                | STRUCT_16
        ) || is_tiny_string(marker)
            || is_tiny_list(marker)
            || is_tiny_map(marker)
            || is_tiny_struct(marker)
    }

    fn read_bytes_data(&mut self, len: usize) -> Result<PackStreamValue, PackStreamError> {
        let bytes = self.read_bytes(len)?;
        Ok(PackStreamValue::Bytes(bytes.to_vec()))
    }

    fn read_string_data(&mut self, len: usize) -> Result<PackStreamValue, PackStreamError> {
        let bytes = self.read_bytes(len)?;
        let s =
            std::str::from_utf8(bytes).map_err(|e| PackStreamError::InvalidUtf8(e.to_string()))?;
        Ok(PackStreamValue::String(s.to_string()))
    }

    fn read_list_data(
        &mut self,
        len: usize,
        depth: usize,
    ) -> Result<PackStreamValue, PackStreamError> {
        let mut items = Vec::with_capacity(len.min(1024));
        for _ in 0..len {
            items.push(self.decode_at_depth(depth + 1)?);
        }
        Ok(PackStreamValue::List(items))
    }

    fn read_map_data(
        &mut self,
        len: usize,
        depth: usize,
    ) -> Result<PackStreamValue, PackStreamError> {
        let mut map = HashMap::with_capacity(len.min(1024));
        for _ in 0..len {
            let key = self.decode_at_depth(depth + 1)?;
            let key_str = match key {
                PackStreamValue::String(s) => s,
                _ => return Err(PackStreamError::InvalidMapKey),
            };
            let value = self.decode_at_depth(depth + 1)?;
            map.insert(key_str, value);
        }
        Ok(PackStreamValue::Map(map))
    }

    fn read_struct_data(
        &mut self,
        field_count: usize,
        depth: usize,
    ) -> Result<PackStreamValue, PackStreamError> {
        let tag = self.read_u8()?;
        let mut fields = Vec::with_capacity(field_count.min(64));
        for _ in 0..field_count {
            fields.push(self.decode_at_depth(depth + 1)?);
        }
        Ok(PackStreamValue::Structure(PackStreamStructure::new(
            tag, fields,
        )))
    }

    // Low-level read methods

    fn read_u8(&mut self) -> Result<u8, PackStreamError> {
        if self.remaining() < 1 {
            return Err(PackStreamError::UnexpectedEof);
        }
        let value = self.data[self.pos];
        self.pos += 1;
        Ok(value)
    }

    fn read_i8(&mut self) -> Result<i8, PackStreamError> {
        Ok(self.read_u8()? as i8)
    }

    fn read_u16(&mut self) -> Result<u16, PackStreamError> {
        if self.remaining() < 2 {
            return Err(PackStreamError::UnexpectedEof);
        }
        let value = (&self.data[self.pos..]).get_u16();
        self.pos += 2;
        Ok(value)
    }

    fn read_i16(&mut self) -> Result<i16, PackStreamError> {
        if self.remaining() < 2 {
            return Err(PackStreamError::UnexpectedEof);
        }
        let value = (&self.data[self.pos..]).get_i16();
        self.pos += 2;
        Ok(value)
    }

    fn read_u32(&mut self) -> Result<u32, PackStreamError> {
        if self.remaining() < 4 {
            return Err(PackStreamError::UnexpectedEof);
        }
        let value = (&self.data[self.pos..]).get_u32();
        self.pos += 4;
        Ok(value)
    }

    fn read_i32(&mut self) -> Result<i32, PackStreamError> {
        if self.remaining() < 4 {
            return Err(PackStreamError::UnexpectedEof);
        }
        let value = (&self.data[self.pos..]).get_i32();
        self.pos += 4;
        Ok(value)
    }

    fn read_i64(&mut self) -> Result<i64, PackStreamError> {
        if self.remaining() < 8 {
            return Err(PackStreamError::UnexpectedEof);
        }
        let value = (&self.data[self.pos..]).get_i64();
        self.pos += 8;
        Ok(value)
    }

    fn read_f64(&mut self) -> Result<f64, PackStreamError> {
        if self.remaining() < 8 {
            return Err(PackStreamError::UnexpectedEof);
        }
        let value = (&self.data[self.pos..]).get_f64();
        self.pos += 8;
        Ok(value)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], PackStreamError> {
        if self.remaining() < len {
            return Err(PackStreamError::UnexpectedEof);
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }
}

/// Convenience function to decode a single value from bytes.
pub fn decode(data: &[u8]) -> Result<PackStreamValue, PackStreamError> {
    let mut decoder = PackStreamDecoder::new(data);
    decoder.decode()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deeply_nested_lists_fail_without_recursing_unboundedly() {
        let mut input = vec![0x91; MAX_NESTING_DEPTH + 2];
        input.push(NULL);
        assert!(matches!(
            decode(&input),
            Err(PackStreamError::NestingTooDeep { .. })
        ));
    }

    #[test]
    fn declared_large_value_with_truncated_bytes_is_stable_eof() {
        assert!(matches!(
            decode(&[BYTES_32, 0xff, 0xff, 0xff, 0xff]),
            Err(PackStreamError::UnexpectedEof)
        ));
    }
}
