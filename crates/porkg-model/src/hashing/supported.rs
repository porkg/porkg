use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    base32::Base32,
    hashing::{StableHash, StableHasher, StableHasherExt},
};

/// Supported hashing algorithms.
#[derive(Debug)]
pub enum SupportedHasher {
    /// Blake3
    Blake3(blake3::Hasher),
}

impl SupportedHasher {
    pub fn blake3() -> Self {
        Self::Blake3(blake3::Hasher::new())
    }

    pub fn update(&mut self, bytes: impl AsRef<[u8]>) {
        match self {
            Self::Blake3(hasher) => hasher.update(bytes.as_ref()),
        };
    }

    pub fn finalize(self) -> SupportedHash {
        match self {
            Self::Blake3(hasher) => SupportedHash::Blake3(*hasher.finalize().as_bytes()),
        }
    }
}

impl StableHasher for SupportedHasher {
    type Result = SupportedHash;

    #[inline(always)]
    fn update(&mut self, bytes: &[u8]) {
        SupportedHasher::update(self, bytes)
    }

    #[inline(always)]
    fn finalize(self) -> Self::Result {
        SupportedHasher::finalize(self)
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SupportedHash {
    Blake3([u8; 32]),
}

impl Ord for SupportedHash {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Blake3(a), Self::Blake3(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for SupportedHash {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

const PREFIX_BLAKE3: &str = "blake3-";

impl std::fmt::Debug for SupportedHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupportedHash::Blake3(h) => write!(f, "Blake3(\"{}\")", Base32(*h)),
        }
    }
}

impl std::fmt::Display for SupportedHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupportedHash::Blake3(h) => write!(f, "blake3-{}", Base32(*h)),
        }
    }
}

impl FromStr for SupportedHash {
    type Err = ParseError<String>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(val) = s.strip_prefix(PREFIX_BLAKE3) {
            let b32: Base32<32> = val.parse().map_err(Into::<ParseError<String>>::into)?;
            Ok(SupportedHash::Blake3(b32.0))
        } else {
            Err(ParseError::UnknownType(s.to_string()))
        }
    }
}

impl SupportedHash {
    pub fn create_matching_hasher(&self) -> SupportedHasher {
        match self {
            SupportedHash::Blake3(_) => SupportedHasher::blake3(),
        }
    }
}

impl StableHash for SupportedHash {
    fn update<H: StableHasher>(&self, h: &mut H) {
        match self {
            SupportedHash::Blake3(hash) => h.update_hash(1u8).update(hash),
        }
    }
}

#[derive(Debug, Error)]
pub enum ParseError<T: std::fmt::Debug> {
    #[error("unknown hash type {:?}", _0)]
    UnknownType(T),
    #[error("expected hash length {:?}", _0)]
    InvalidLength(usize),
    #[error("invalid base32 value")]
    InvalidBase32,
}

impl<T: std::fmt::Debug> From<crate::base32::InvalidBase32> for ParseError<T> {
    fn from(_: crate::base32::InvalidBase32) -> Self {
        Self::InvalidBase32
    }
}
