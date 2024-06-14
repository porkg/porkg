//! A capacity-limited object pool
//!
//! Provides an object pool that is wait-free. The pool contains a maximum number of objects. If more objects are
//! returned than the pool can contain, they will be dropped immediately.
//!
//! There are no ordering guarantees.

use std::sync::atomic::{AtomicUsize, Ordering};

mod owned_pooled_item;
mod pooled_item;
use bytes::BytesMut;
use flume::TrySendError;
use nix::unistd::gettid;
use once_cell::sync::{Lazy, OnceCell};
pub use owned_pooled_item::OwnedPooled;
pub use pooled_item::Pooled;

const MB: usize = 131072;
const MAX_TOTAL_BUFFERS: usize = 128 * MB;
const MAX_SINGLE_BUFFER: usize = 16 * MB;
const DEFAULT_BUFFER_LEN: usize = 16384;

static CURRENT_SIZE: AtomicUsize = AtomicUsize::new(0);
static BUFFER_POOL: Pool<'static, BytesMut> = PoolBuilder::<BytesMut>::new(16)
    .with_max_search(8)
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
    .build(|| BytesMut::with_capacity(DEFAULT_BUFFER_LEN));

/// Gets a pooled memory buffer.
pub fn get_buffer() -> Pooled<'static, BytesMut> {
    BUFFER_POOL.take()
}

/// The return portion of a pool.
pub trait PoolReturn<T>: Sync + crate::sealed::Sealed {
    /// Returns a value to the pool.
    fn return_value(&self, value: T);
}

struct PoolEntry<T> {
    sender: flume::Sender<T>,
    receiver: flume::Receiver<T>,
}

/// A builder for a `Pool`.
pub struct PoolBuilder<'a, T: Send> {
    return_hook: Option<&'a (dyn Sync + Fn(T) -> Option<T>)>,
    take_hook: Option<&'a (dyn Sync + Fn(T) -> T)>,
    max_loop: usize,
    buckets: usize,
    capacity: usize,
}

impl<'a, T: Send> PoolBuilder<'a, T> {
    /// Creates a pool builder with a capacity.
    ///
    /// # Panics
    ///
    /// When `capacity` is 0.
    pub const fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "must have capacity for at least one item");
        Self {
            return_hook: None,
            take_hook: None,
            max_loop: capacity,
            buckets: 0,
            capacity,
        }
    }

    /// Sets the number of buckets that reduce contention.
    pub const fn with_buckets(mut self, buckets: usize) -> Self {
        self.buckets = buckets;
        self
    }

    /// Sets a hook that can filter and mutate values as they are being returned to the pool.
    pub const fn with_return_hook(
        mut self,
        return_hook: &'a (impl Sync + Fn(T) -> Option<T>),
    ) -> Self {
        self.return_hook = Some(return_hook);
        self
    }

    /// Sets a hook that can mutate values when they are taken from the pool.
    ///
    /// This hook will not run when a value is created, only when an value is found in the pool and is returned.
    pub const fn with_take_hook(mut self, take_hook: &'a (impl Sync + Fn(T) -> T)) -> Self {
        self.take_hook = Some(take_hook);
        self
    }

    /// Sets the maximum amount of times a free slot should be searched for.
    pub const fn with_max_search(mut self, max_loop: usize) -> Self {
        self.max_loop = max_loop;
        self
    }

    /// Creates an object pool with the given factory.
    ///
    /// The object bool will use `create` when a new instance of the object is required.
    pub const fn build<F: Sync + Send + Fn() -> T>(self, create: F) -> Pool<'a, T, F> {
        Pool {
            state: OnceCell::new(),
            config: self,
            create,
        }
    }
}

struct PoolState<'a, T: Send, F: Sync + Send + Fn() -> T> {
    skip: AtomicUsize,
    entries: Box<[PoolEntry<T>]>,
    config: &'a PoolBuilder<'a, T>,
    create: &'a F,
}

static DEFAULT_BUCKETS: Lazy<usize> = Lazy::new(|| {
    if let Some(result) = std::env::var("PORKG_MEM_BUCKETS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        if result > 0 {
            return result;
        }
    }

    if let Ok(cores) = std::thread::available_parallelism() {
        return cores.into();
    }

    16
});

impl<'a, T: Send, F: Sync + Send + Fn() -> T> PoolState<'a, T, F> {
    fn new(config: &'a PoolBuilder<'a, T>, create: &'a F) -> Self {
        let mut buckets = config.buckets;
        if buckets == 0 {
            buckets = *DEFAULT_BUCKETS;
        }
        buckets = buckets.max(config.capacity);

        let mut entries = Vec::with_capacity(buckets);
        let per_bucket = config.capacity / buckets;
        for _ in 0..(buckets - 1) {
            let (sender, receiver) = flume::bounded(per_bucket);
            entries.push(PoolEntry { sender, receiver });
        }

        let (sender, receiver) = flume::bounded(config.capacity - per_bucket * entries.len());
        entries.push(PoolEntry { sender, receiver });

        PoolState {
            skip: AtomicUsize::new(0),
            entries: entries.into_boxed_slice(),
            config,
            create,
        }
    }

    fn next_id(&self) -> usize {
        let id = gettid().as_raw() as usize;
        self.skip.fetch_add(id, Ordering::Relaxed)
    }

    pub fn take(&self) -> T {
        let id = self.next_id();
        for i in 0..=self.config.max_loop {
            let i = i.wrapping_add(id).wrapping_rem(self.entries.len());

            let result = if let Ok(v) = self.entries[i].receiver.try_recv() {
                v
            } else {
                continue;
            };

            return if let Some(hook) = self.config.take_hook {
                hook(result)
            } else {
                result
            };
        }
        (self.create)()
    }
}

impl<'a, T: Send, F: Sync + Send + Fn() -> T> crate::sealed::Sealed for PoolState<'a, T, F> {}

impl<'a, T: Send, F: Sync + Send + Fn() -> T> PoolReturn<T> for PoolState<'a, T, F> {
    #[inline]
    fn return_value(&self, value: T) {
        let mut value = if let Some(hook) = self.config.return_hook {
            if let Some(value) = hook(value) {
                value
            } else {
                return;
            }
        } else {
            value
        };

        let id = self.next_id();
        for i in 0..=self.config.max_loop {
            let i = i.wrapping_add(id).wrapping_rem(self.entries.len());
            match self.entries[i].sender.try_send(value) {
                Ok(_) => break,
                Err(TrySendError::Disconnected(_)) => break,
                Err(TrySendError::Full(e)) => value = e,
            }
        }
    }
}

/// A wait-free capacity-limited pool of objects.
///
/// These can be created with the [`PoolBuilder<T, F>`].
pub struct Pool<'a, T: Send, F: Sync + Send + Fn() -> T = fn() -> T> {
    state: OnceCell<PoolState<'a, T, F>>,
    config: PoolBuilder<'a, T>,
    create: F,
}

impl<'a, T: Send, F: Sync + Send + Fn() -> T> Pool<'a, T, F> {
    fn state(&'a self) -> &PoolState<'a, T, F> {
        self.state
            .get_or_init(|| PoolState::new(&self.config, &self.create))
    }

    /// Gets the capacity.
    pub const fn capacity(&self) -> usize {
        self.config.capacity
    }

    /// Takes or creates a single pooled value.
    ///
    /// The returned object will return the value to the pool when dropped.
    #[inline]
    pub fn take(&'a self) -> Pooled<'a, T> {
        let state = self.state();
        let result = state.take();
        Pooled::new(result, state)
    }
}

const NULL_POOL: NullPool = NullPool;
struct NullPool;

impl crate::sealed::Sealed for NullPool {}
impl<T> PoolReturn<T> for NullPool {
    #[inline]
    fn return_value(&self, _: T) {}
}

#[cfg(test)]
mod test {
    use std::{
        hint::black_box,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc::channel,
            Arc,
        },
    };

    use once_cell::sync::Lazy;

    use crate::mem::Pooled;

    use super::{Pool, PoolBuilder, PoolReturn as _};

    #[test]
    pub fn take_return() {
        let pool = PoolBuilder::new(16).build(|| 0u64);
        pool.take();
    }

    static COUNTER_THREADED: AtomicU64 = AtomicU64::new(0);
    static POOL_THREADED: Lazy<Pool<Box<u64>>> = Lazy::new(|| {
        PoolBuilder::<Box<u64>>::new(16)
            .build(|| Box::new(COUNTER_THREADED.fetch_add(1, std::sync::atomic::Ordering::SeqCst)))
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

        for _ in 0..100 {
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
            for _ in 0..200 {
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
    pub fn return_hook() {
        let pool = PoolBuilder::<u64>::new(16)
            .with_return_hook(&|v| {
                if v.wrapping_rem(2) == 0 {
                    Some(v)
                } else {
                    None
                }
            })
            .build(|| 0);

        for i in 0..(pool.capacity() * 2) {
            pool.state().return_value(i as u64);
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
        let pool = PoolBuilder::<u64>::new(16)
            .with_take_hook(&|_| COUNTER.fetch_add(1, std::sync::atomic::Ordering::AcqRel))
            .build(|| 0);

        for _ in 0..(pool.capacity() * 2) {
            pool.state().return_value(0);
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

    #[test]
    pub fn forget() {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let pool = PoolBuilder::<u64>::new(16).build(|| COUNTER.fetch_add(1, Ordering::SeqCst));
        black_box(pool.take().forget());
        for _ in 0..pool.capacity() {
            assert!(*pool.take().as_ref() > 1);
        }
    }
}
