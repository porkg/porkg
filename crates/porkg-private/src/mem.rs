//! A capacity-limited object pool
//!
//! Provides an object pool that is wait-free. The pool contains a maximum number of objects. If more objects are
//! returned than the pool can contain, they will be dropped immediately.
//!
//! There are no ordering guarantees.

use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
};

mod owned_pooled_item;
mod pooled_item;
use bytes::BytesMut;
use nix::unistd::gettid;
use once_cell::sync::Lazy;
pub use owned_pooled_item::OwnedPooled;
pub use pooled_item::Pooled;

const MB: usize = 131072;
const MAX_TOTAL_BUFFERS: usize = 128 * MB;
const MAX_SINGLE_BUFFER: usize = 16 * MB;
const DEFAULT_BUFFER_LEN: usize = 16384;

static CURRENT_SIZE: AtomicUsize = AtomicUsize::new(0);
pub static BUFFER_POOL: Lazy<Pool<'static, 128, BytesMut>> = Lazy::new(|| {
    Pool::new(&|| BytesMut::with_capacity(DEFAULT_BUFFER_LEN))
        .with_max_search(16)
        .with_take_hook(&|v| {
            CURRENT_SIZE.fetch_sub(v.capacity(), std::sync::atomic::Ordering::Release);
            v
        })
        .with_return_hook(&|mut v| {
            let capacity = v.capacity();
            if capacity > MAX_SINGLE_BUFFER
                || CURRENT_SIZE.load(std::sync::atomic::Ordering::Acquire) + capacity
                    > MAX_TOTAL_BUFFERS
            {
                None
            } else {
                // It's a soft limit.
                CURRENT_SIZE.fetch_add(capacity, std::sync::atomic::Ordering::Release);
                v.clear();
                Some(v)
            }
        })
});

const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const AVAILABLE: u8 = 2;
const TAKING: u8 = 3;
const DESTROYED: u8 = 4;

/// The return portion of a pool.
pub trait PoolReturn<T>: Sync {
    /// Returns a value to the pool.
    fn return_value(&self, value: T);
}

struct PoolEntry<T> {
    item: UnsafeCell<MaybeUninit<T>>,
    state: AtomicU8,
}

impl<T> Default for PoolEntry<T> {
    fn default() -> Self {
        Self {
            item: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(0),
        }
    }
}

impl<T> Clone for PoolEntry<T> {
    fn clone(&self) -> Self {
        Default::default()
    }
}

struct PoolState<'a, T, const CAPACITY: usize> {
    skip: AtomicUsize,
    entries: Box<[PoolEntry<T>; CAPACITY]>,
    create: &'a dyn Fn() -> T,
    return_hook: Option<&'a dyn Fn(T) -> Option<T>>,
    take_hook: Option<&'a dyn Fn(T) -> T>,
    max_loop: usize,
}

unsafe impl<'a, T, const CAPACITY: usize> Sync for PoolState<'a, T, CAPACITY> {}
unsafe impl<'a, T: Send, const CAPACITY: usize> Send for PoolState<'a, T, CAPACITY> {}

/// The actual implementation of the pool.
///
/// It is implemented as a array of objects and states. When retrieving a value the reader will attempt to update the
/// corresponding state from `AVAILABLE` to `TAKING`. If that succeeds it can then take the value and write `EMPTY`.
/// Likewise, when returning a value the writer will transition from `EMPTY`, to `WRITING`, to `AVAILABLE`. If a the
/// initial lock fails (`AVAILABLE` to `TAKING`, or `EMPTY` to `WRITING`) it will give up on the cell and immediately
/// move to the next. Contention is reduced with a global shared counter that is used to determine a wrapping offest
/// into the array to start at.
///
/// This does not read then CAS, so does not suffer from ABA.
impl<'a, T, const CAPACITY: usize> PoolState<'a, T, CAPACITY> {
    fn next_id(&self) -> usize {
        let id = gettid().as_raw() as usize;
        self.skip.fetch_add(id, Ordering::Relaxed)
    }

    pub fn take(&self) -> T {
        let id = self.next_id();
        for i in 0..self.max_loop {
            let i = i.wrapping_add(id).wrapping_rem(CAPACITY);

            if self.entries[i]
                .state
                .compare_exchange(AVAILABLE, TAKING, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }

            let cell = unsafe { &mut *self.entries[i].item.get() };
            let val = std::mem::replace(cell, MaybeUninit::uninit());
            self.entries[i]
                .state
                .compare_exchange(TAKING, EMPTY, Ordering::Release, Ordering::Relaxed)
                .ok();
            let result = unsafe { val.assume_init() };
            return if let Some(hook) = self.take_hook {
                hook(result)
            } else {
                result
            };
        }
        (self.create)()
    }
}

impl<'a, T, const CAPACITY: usize> PoolReturn<T> for PoolState<'a, T, CAPACITY> {
    #[inline]
    fn return_value(&self, value: T) {
        let value = if let Some(hook) = self.return_hook {
            if let Some(value) = hook(value) {
                value
            } else {
                return;
            }
        } else {
            value
        };

        let id = self.next_id();
        for i in 0..self.max_loop {
            let i = i.wrapping_add(id).wrapping_rem(CAPACITY);

            match self.entries[i].state.compare_exchange(
                EMPTY,
                WRITING,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {}
                Err(DESTROYED) => break,
                _ => continue,
            }

            let cell = unsafe { &mut *self.entries[i].item.get() };
            *cell = MaybeUninit::new(value);
            self.entries[i]
                .state
                .compare_exchange(WRITING, AVAILABLE, Ordering::Release, Ordering::Relaxed)
                .ok();
            break;
        }
    }
}

impl<'a, T, const CAPACITY: usize> Drop for PoolState<'a, T, CAPACITY> {
    fn drop(&mut self) {
        // Given the mut reference this is probably completely unessecary
        let mut except = CAPACITY;
        while except != 0 {
            except = CAPACITY;
            for i in 0..CAPACITY {
                if self.entries[i].state.load(Ordering::Acquire) == DESTROYED {
                    except -= 1;
                    continue;
                }

                match self.entries[i].state.compare_exchange(
                    EMPTY,
                    DESTROYED,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) | Err(DESTROYED) => {
                        except -= 1;
                        continue;
                    }
                    // We have to wait for writing to complete
                    Err(WRITING) => std::thread::yield_now(),
                    _ => {}
                }

                match self.entries[i].state.compare_exchange(
                    AVAILABLE,
                    DESTROYED,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        let val = std::mem::replace(
                            self.entries[i].item.get_mut(),
                            MaybeUninit::uninit(),
                        );
                        drop(unsafe { val.assume_init() });
                        except -= 1;
                        continue;
                    }
                    Err(DESTROYED) => {
                        except -= 1;
                        continue;
                    }
                    Err(WRITING) | Err(EMPTY) => continue,
                    _ => {}
                }

                // We don't need to wait for TAKING to finish because it will be returned at some point
                match self.entries[i].state.compare_exchange(
                    TAKING,
                    DESTROYED,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) | Err(DESTROYED) => except -= 1,
                    _ => {}
                }
            }

            // Allow the threads we are waiting on to proceed
            std::thread::yield_now();
        }
    }
}

/// A wait-free capacity-limited pool of objects.
///
/// * `CAPACITY`: The capacity of the pool.
/// * `T`: The type of object in the pool.
pub struct Pool<'a, const CAPACITY: usize, T> {
    state: PoolState<'a, T, CAPACITY>,
}

impl<'a, const CAPACITY: usize, T> Pool<'a, CAPACITY, T> {
    /// Creates a new `Pool`.
    ///
    /// # Parameters
    ///
    /// * `create`: A factory for `T`.
    pub fn new(create: &'a (impl Send + Sync + Fn() -> T)) -> Self {
        let state = PoolState {
            skip: AtomicUsize::new(0),
            entries: vec![PoolEntry::default(); CAPACITY]
                .into_boxed_slice()
                .try_into()
                .unwrap_or_else(|_| unreachable!()),
            create,
            return_hook: None,
            take_hook: None,
            max_loop: CAPACITY,
        };
        Self { state }
    }

    /// Creates a new `Pool` wrapped in a `Lazy`.
    ///
    /// # Parameters
    ///
    /// * `create`: A factory for `T`.
    pub fn new_lazy(create: &'a (impl Send + Sync + Fn() -> T)) -> Lazy<Self, impl Fn() -> Self> {
        Lazy::new(|| Self::new(create))
    }

    /// Sets a hook that can filter and mutate values as they are being returned to the pool.
    pub const fn with_return_hook(mut self, return_hook: &'a impl Fn(T) -> Option<T>) -> Self {
        self.state.return_hook = Some(return_hook);
        self
    }

    /// Sets a hook that can mutate values when they are taken from the pool.
    ///
    /// This hook will not run when a value is created, only when an value is found in the pool and is returned.
    pub const fn with_take_hook(mut self, take_hook: &'a impl Fn(T) -> T) -> Self {
        self.state.take_hook = Some(take_hook);
        self
    }

    /// Sets the maximum amount of times a free slot should be searched for.
    pub const fn with_max_search(mut self, max_loop: usize) -> Self {
        // min is not const
        if max_loop <= CAPACITY {
            self.state.max_loop = max_loop;
        } else {
            self.state.max_loop = CAPACITY;
        }
        self
    }

    /// Gets the value of `CAPACITY`.
    pub const fn capacity(&self) -> usize {
        CAPACITY
    }

    /// Takes or creates a single pooled value.
    ///
    /// The returned object will return the value to the pool when dropped.
    #[inline]
    pub fn take(&self) -> Pooled<'_, T> {
        let result = self.state.take();
        Pooled::new(result, &self.state)
    }
}

const NULL_POOL: NullPool = NullPool;
struct NullPool;

impl<T> PoolReturn<T> for NullPool {
    #[inline]
    fn return_value(&self, _: T) {}
}

#[cfg(test)]
mod test {
    use std::{
        hint::black_box,
        sync::{
            atomic::{AtomicBool, AtomicU64},
            mpsc::channel,
            Arc,
        },
    };

    use once_cell::sync::Lazy;

    use crate::mem::{Pooled, EMPTY};

    use super::{Pool, PoolReturn};

    #[test]
    pub fn take_return() {
        let pool = Pool::<16, _>::new(&|| 0u64);
        pool.take();
    }

    static COUNTER_THREADED: AtomicU64 = AtomicU64::new(0);
    static POOL_THREADED: Lazy<Pool<16, Box<u64>>> = Lazy::new(|| {
        Pool::<16, Box<u64>>::new(&|| {
            Box::new(COUNTER_THREADED.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
        })
    });

    #[test]
    pub fn threaded() {
        let (send, receive) = channel::<Pooled<Box<u64>>>();
        let running = Arc::new(AtomicBool::new(true));

        let handle = std::thread::spawn(move || {
            while let Ok(v) = receive.recv() {
                black_box(v);
            }
        });

        for _ in 0..3 {
            // Need more than one writer
            let running = running.clone();
            std::thread::spawn(move || {
                while running.load(std::sync::atomic::Ordering::Acquire) {
                    std::thread::yield_now();
                    black_box(POOL_THREADED.take());
                }
            });
        }

        std::thread::scope(|s| {
            for _ in 0..20 {
                s.spawn(|| {
                    let send = send.clone();
                    for _ in 0..100 {
                        let v = POOL_THREADED.take();
                        send.send(v).unwrap();
                    }
                });
            }
        });
        drop(send);
        running.store(false, std::sync::atomic::Ordering::Release);

        handle.join().unwrap();
    }

    #[test]
    pub fn forget() {
        let pool = Pool::<16, u64>::new(&|| 0);
        black_box(pool.take().forget());
        for i in 0..pool.capacity() {
            assert_eq!(
                EMPTY,
                pool.state.entries[i]
                    .state
                    .load(std::sync::atomic::Ordering::SeqCst)
            );
        }
    }

    #[test]
    pub fn return_hook() {
        let pool = Pool::<16, u64>::new(&|| 0).with_return_hook(&|v| {
            if v.wrapping_rem(2) == 0 {
                Some(v)
            } else {
                None
            }
        });

        for i in 0..(pool.capacity() * 2) {
            pool.state.return_value(i as u64);
        }

        for _ in 0..pool.capacity() {
            let v = pool.take();
            assert!(v.wrapping_rem(2) == 0);
            v.forget();
        }
    }

    #[test]
    pub fn take_hook() {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let pool = Pool::<16, u64>::new(&|| 0)
            .with_take_hook(&|_| COUNTER.fetch_add(1, std::sync::atomic::Ordering::AcqRel));

        for _ in 0..(pool.capacity() * 2) {
            pool.state.return_value(0);
        }

        for _ in 0..pool.capacity() {
            let v = pool.take();
            assert_ne!(v.forget(), 0);
        }

        for _ in 0..pool.capacity() {
            let v = pool.take();
            assert_eq!(0, v.forget());
        }
    }
}
