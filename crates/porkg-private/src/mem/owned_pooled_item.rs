use std::{
    hash::Hash,
    ops::{Deref, DerefMut},
};

use super::PoolReturn;

/// A value that was retrieved from a `Pool`.
pub struct OwnedPooled<T, P: PoolReturn<T> = Box<dyn PoolReturn<T>>> {
    value: Option<T>,
    pool: P,
}

impl<T, P: PoolReturn<T>> OwnedPooled<T, P> {
    /// Creates a new pooled item.
    pub fn new(value: T, pool: P) -> Self {
        Self {
            value: Some(value),
            pool,
        }
    }

    /// Gets a reference to the value.
    ///
    #[inline]
    pub fn get(&self) -> &T {
        self.value.as_ref().unwrap()
    }

    /// Gets a mutable reference to the value.
    ///
    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.value.as_mut().unwrap()
    }

    /// Forgets the contained value
    ///
    /// Prevents the contained value from being returned to the pool, and returns it to the caller.
    pub fn forget(mut self) -> T {
        self.value.take().unwrap()
    }
}

impl<T: std::fmt::Debug, P: PoolReturn<T>> std::fmt::Debug for OwnedPooled<T, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.value.as_ref() {
            Some(v) => v.fmt(f),
            None => f.debug_tuple("EmptyPooledItem").finish(),
        }
    }
}

impl<T, P: PoolReturn<T>> Deref for OwnedPooled<T, P> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.value.as_ref().unwrap()
    }
}

impl<T, P: PoolReturn<T>> DerefMut for OwnedPooled<T, P> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().unwrap()
    }
}

impl<T, P: PoolReturn<T>> AsRef<T> for OwnedPooled<T, P> {
    #[inline]
    fn as_ref(&self) -> &T {
        self.value.as_ref().unwrap()
    }
}

impl<T, P: PoolReturn<T>> AsMut<T> for OwnedPooled<T, P> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        self.value.as_mut().unwrap()
    }
}

impl<T: PartialEq, P: PoolReturn<T>> PartialEq for OwnedPooled<T, P> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

impl<T: Eq, P: PoolReturn<T>> Eq for OwnedPooled<T, P> {}

impl<T: Hash, P: PoolReturn<T>> Hash for OwnedPooled<T, P> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state)
    }
}

impl<T, P: PoolReturn<T>> Drop for OwnedPooled<T, P> {
    #[inline]
    fn drop(&mut self) {
        if let Some(old) = self.value.take() {
            self.pool.return_value(old)
        }
    }
}
