//! Canonical binary serialization for Zeta4j persisted and wire payloads.
//!
//! One strict configuration avoids format drift between crates. Decoding rejects
//! trailing bytes and bounds the encoded input before decoding. Callers with a
//! narrower public-input boundary use the const-generic bounded functions.

use postcard::Error;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Default ceiling for internal persisted values.
pub const DEFAULT_LIMIT_BYTES: usize = 1024 * 1024 * 1024;

/// Serialize with the canonical Postcard format.
pub fn serialize<T>(value: &T) -> Result<Vec<u8>, Error>
where
    T: Serialize + ?Sized,
{
    serialize_bounded::<DEFAULT_LIMIT_BYTES, T>(value)
}

/// Serialize while enforcing a caller-owned byte ceiling.
pub fn serialize_bounded<const LIMIT: usize, T>(value: &T) -> Result<Vec<u8>, Error>
where
    T: Serialize + ?Sized,
{
    let encoded = postcard::to_stdvec(value)?;
    if encoded.len() > LIMIT {
        return Err(Error::SerializeBufferFull);
    }
    Ok(encoded)
}

/// Deserialize one complete value with the default ceiling.
pub fn deserialize<T>(bytes: &[u8]) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    deserialize_bounded::<DEFAULT_LIMIT_BYTES, T>(bytes)
}

/// Deserialize one complete value under a caller-owned byte ceiling.
pub fn deserialize_bounded<const LIMIT: usize, T>(bytes: &[u8]) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let (value, remaining) = decode_bounded::<LIMIT, T>(bytes)?;
    if !remaining.is_empty() {
        return Err(Error::DeserializeBadEncoding);
    }
    Ok(value)
}

/// Deserialize a prefix value when the containing format owns the remaining bytes.
pub fn deserialize_allow_trailing_bounded<const LIMIT: usize, T>(bytes: &[u8]) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    decode_bounded::<LIMIT, T>(bytes).map(|(value, _)| value)
}

fn decode_bounded<const LIMIT: usize, T>(bytes: &[u8]) -> Result<(T, &[u8]), Error>
where
    T: DeserializeOwned,
{
    if bytes.len() > LIMIT {
        return Err(Error::DeserializeBadEncoding);
    }
    postcard::take_from_bytes(bytes)
}

/// Return the canonical serialized size.
pub fn serialized_size<T>(value: &T) -> Result<u64, Error>
where
    T: Serialize + ?Sized,
{
    serialize(value).map(|encoded| encoded.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_uses_one_canonical_format() -> Result<(), Box<dyn std::error::Error>> {
        let value = (7_u64, "zeta4j".to_owned(), vec![1_u8, 2, 3]);
        let encoded = serialize(&value)?;
        let decoded: (u64, String, Vec<u8>) = deserialize(&encoded)?;
        assert_eq!(decoded, value);
        assert_eq!(serialized_size(&value)?, encoded.len() as u64);
        Ok(())
    }

    #[test]
    fn complete_decode_rejects_trailing_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let mut encoded = serialize(&41_u64)?;
        encoded.push(0);
        assert!(deserialize::<u64>(&encoded).is_err());
        assert_eq!(deserialize_allow_trailing_bounded::<64, u64>(&encoded)?, 41);
        Ok(())
    }

    #[test]
    fn bounded_decode_rejects_oversized_declared_collection()
    -> Result<(), Box<dyn std::error::Error>> {
        let encoded = serialize(&vec![0_u8; 32])?;
        assert!(deserialize_bounded::<16, Vec<u8>>(&encoded).is_err());
        Ok(())
    }
}
