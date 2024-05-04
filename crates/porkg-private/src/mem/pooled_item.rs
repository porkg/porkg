use std::{
    hash::Hash,
    ops::{Deref, DerefMut},
};

use super::{PoolReturn, NULL_POOL};

/// A value that was retrieved from a `Pool`.
pub struct Pooled<'a, T> {
    value: Option<T>,
    pool: &'a dyn PoolReturn<T>,
}

impl<'a, T> Pooled<'a, T> {
    /// Creates a new pooled item.
    pub fn new(value: T, pool: &'a impl PoolReturn<T>) -> Self {
        Self {
            value: Some(value),
            pool,
        }
    }

    /// Gets a reference to the value.
    ///
    #[inline]
    pub fn get(&'a self) -> &'a T {
        self.value.as_ref().unwrap()
    }

    /// Gets a mutable reference to the value.
    ///
    #[inline]
    pub fn get_mut(&'a mut self) -> &'a mut T {
        self.value.as_mut().unwrap()
    }

    /// Forgets the contained value
    ///
    /// Prevents the contained value from being returned to the pool, and returns it to the caller.
    pub fn forget(mut self) -> T {
        self.value.take().unwrap()
    }

    /// Applies the given function to the contained value.
    pub fn apply_result<E>(mut self, f: impl FnOnce(T) -> Result<T, E>) -> Result<Self, E> {
        let value = self.value.take().unwrap();
        match f(value) {
            Ok(value) => {
                self.value = Some(value);
                Ok(self)
            }
            Err(e) => Err(e),
        }
    }
}

impl<'a, T: std::fmt::Debug> std::fmt::Debug for Pooled<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.value.as_ref() {
            Some(v) => v.fmt(f),
            None => f.debug_tuple("EmptyPooledItem").finish(),
        }
    }
}

impl<'a, T> Deref for Pooled<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.value.as_ref().unwrap()
    }
}

impl<'a, T> DerefMut for Pooled<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().unwrap()
    }
}

impl<'a, T> AsRef<T> for Pooled<'a, T> {
    #[inline]
    fn as_ref(&self) -> &T {
        self.value.as_ref().unwrap()
    }
}

impl<'a, T> AsMut<T> for Pooled<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        self.value.as_mut().unwrap()
    }
}

impl<'a, T: PartialEq> PartialEq for Pooled<'a, T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.value.eq(&other.value)
    }
}

impl<'a, T: Eq> Eq for Pooled<'a, T> {}

impl<'a, T: Hash> Hash for Pooled<'a, T> {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state)
    }
}

impl<'a, T: Clone + Copy> Clone for Pooled<'a, T> {
    #[inline]
    fn clone(&self) -> Self {
        Pooled {
            value: self.value,
            pool: &NULL_POOL,
        }
    }
}

impl<'a, T> Drop for Pooled<'a, T> {
    #[inline]
    fn drop(&mut self) {
        if let Some(old) = self.value.take() {
            self.pool.return_value(old)
        }
    }
}
