mod supported;

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
};

pub use supported::*;

/// A hashing mechanism that is stable.
pub trait StableHasher: Sized {
    /// The type of hash that the hasher produces.
    type Result;

    /// Update the hash with the given bytes.
    fn update(&mut self, bytes: &[u8]);

    /// Finalize the hash.
    fn finalize(self) -> Self::Result;
}

pub trait StableHasherExt: StableHasher {
    /// Incorporate the given value into the hash.
    #[inline(always)]
    fn update_hash<H: StableHash>(&mut self, v: H) -> &mut Self {
        v.update(self);
        self
    }

    /// Incorporate the length and elements of the given iterator into the hash.
    ///
    /// Don't use this on types where the order is not deterministic, such as [`std::collections::HashMap`]. Use
    /// collections with stable ordering instead, such as [`std::collections::BTreeMap`].
    fn update_iter<H: StableHash>(&mut self, v: impl Iterator<Item = H>) -> &mut Self {
        for (i, v) in v.enumerate() {
            self.update_hash(i as u64);
            v.update(self);
        }
        self.update_hash(u64::MAX);
        self
    }
}

impl<H: StableHasher> StableHasherExt for H {}

/// A trait which allows a value to be hashed.
pub trait StableHash {
    fn update<H: StableHasher>(&self, h: &mut H);
}

pub trait StableHashExt: StableHash + Sized {
    /// Calculate the hash of the value.
    #[inline(always)]
    fn hash<H: StableHasher>(&self, mut h: H) -> H::Result {
        h.update_hash(self);
        h.finalize()
    }
}

impl<T: StableHash + Sized> StableHashExt for T {}

impl<T: StableHash> StableHash for &T {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        (*self).update(h)
    }
}

macro_rules! impl_simple {
    ($ty: ident) => {
        impl StableHash for $ty {
            #[inline(always)]
            fn update<H: StableHasher>(&self, h: &mut H) {
                h.update(&self.to_be_bytes());
            }
        }
    };
}

impl_simple!(u16);
impl_simple!(u32);
impl_simple!(u64);
impl_simple!(u128);

impl_simple!(i16);
impl_simple!(i32);
impl_simple!(i64);
impl_simple!(i128);

impl StableHash for bool {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        if *self {
            h.update(b"\xFF");
        } else {
            h.update(b"\x00");
        }
    }
}

impl StableHash for u8 {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        h.update(&[*self]);
    }
}

impl StableHash for i8 {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        h.update(&[*self as u8]);
    }
}

impl StableHash for usize {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        (*self as u64).update(h);
    }
}

impl StableHash for isize {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        (*self as i64).update(h);
    }
}

impl StableHash for OsString {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        self.as_os_str().as_bytes().update(h)
    }
}

impl StableHash for &OsStr {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        self.as_bytes().update(h)
    }
}

impl StableHash for PathBuf {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        self.as_os_str().as_bytes().update(h)
    }
}

impl StableHash for &Path {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        self.as_os_str().as_bytes().update(h)
    }
}

impl StableHash for String {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        self.as_bytes().update(h)
    }
}

impl StableHash for &str {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        self.as_bytes().update(h)
    }
}

impl<T: StableHash> StableHash for Vec<T> {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        self.as_slice().update(h)
    }
}

impl<T: StableHash> StableHash for [T] {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        h.update_hash(self.len());
        for i in self.iter() {
            i.update(h);
        }
    }
}

impl<T: StableHash> StableHash for Option<T> {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        match self {
            Some(v) => h.update_hash(0xFFu8).update_hash(v),
            None => h.update_hash(0u8),
        };
    }
}

// Not implemented for Set and Map because the order of those is undefined

impl<T: StableHash> StableHash for BTreeSet<T> {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        h.update_iter(self.iter());
    }
}

impl<K: StableHash, V: StableHash> StableHash for BTreeMap<K, V> {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        h.update_iter(self.iter());
    }
}

impl<T1: StableHash, T2: StableHash> StableHash for (T1, T2) {
    #[inline(always)]
    fn update<H: StableHasher>(&self, h: &mut H) {
        h.update_hash(&self.0).update_hash(&self.1);
    }
}
