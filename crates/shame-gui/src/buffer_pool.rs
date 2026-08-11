//! Generic best-fit buffer pool: recycles dropped buffers so that
//! allocations happen once and the runtime keeps a small set of live
//! buffers at various sizes.
//!
//! ## How it works
//!
//! - `acquire(needed, create)` searches for the smallest free buffer
//!   with size ≥ `needed` (best-fit via `BTreeMap::range`). If none is
//!   found, `create` is called to allocate a new buffer.
//! - `release(buf, size)` returns a buffer to the pool keyed by its
//!   actual size so that larger buffers can satisfy smaller future
//!   requests.
//! - The pool is `!Send + !Sync` by design (single-threaded use). Wrap
//!   in `Rc<RefCell<>>` when shared across closures.

use std::collections::BTreeMap;

/// A generic best-fit buffer pool. `T` is the buffer handle type
/// (e.g. `Vec<u8>` for CPU buffers, `wgpu::Buffer` for GPU buffers).
///
/// Buffers are keyed by their *actual* size in bytes, not by the
/// requested size. This means a 64KB buffer that was released can
/// satisfy a later 12KB request.
pub struct BufferPool<T> {
    free: BTreeMap<u64, Vec<T>>,
}

impl<T> BufferPool<T> {
    pub fn new() -> Self {
        Self {
            free: BTreeMap::new(),
        }
    }

    /// Get a buffer with at least `needed` bytes.
    ///
    /// Tries the pool first (best-fit: smallest buffer ≥ `needed`).
    /// Falls back to calling `create(needed)` if the pool has no
    /// suitable buffer.
    pub fn acquire(&mut self, needed: u64, create: impl FnOnce(u64) -> T) -> T {
        if let Some((&size, list)) = self.free.range_mut(needed..).next() {
            if let Some(buf) = list.pop() {
                if list.is_empty() {
                    self.free.remove(&size);
                }
                return buf;
            }
        }
        create(needed)
    }

    /// Return a buffer to the pool keyed by its actual `size` in bytes.
    pub fn release(&mut self, buf: T, size: u64) {
        self.free.entry(size).or_default().push(buf);
    }

    /// Total number of buffers currently in the pool.
    pub fn len(&self) -> usize {
        self.free.values().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.free.is_empty()
    }

    /// Discard all pooled buffers, freeing their underlying resources.
    pub fn clear(&mut self) {
        self.free.clear();
    }
}

impl<T> Default for BufferPool<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::fmt::Debug for BufferPool<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sizes: Vec<_> = self
            .free
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(s, v)| (*s, v.len()))
            .collect();
        f.debug_struct("BufferPool").field("free", &sizes).finish()
    }
}

// ── Specialized for Vec<u8> ─────────────────────────────────────────────

impl BufferPool<Vec<u8>> {
    /// Acquire a `Vec<u8>` with at least `needed` capacity. The
    /// returned vec is empty (len = 0) but retains the capacity.
    pub fn acquire_vec(&mut self, needed: usize) -> Vec<u8> {
        let mut v = self.acquire(needed as u64, |n| Vec::with_capacity(n as usize));
        v.clear();
        v
    }

    /// Return a `Vec<u8>` to the pool. The vec is cleared (len = 0) and
    /// stored keyed by its capacity.
    pub fn release_vec(&mut self, mut vec: Vec<u8>) {
        vec.clear();
        let cap = vec.capacity() as u64;
        self.release(vec, cap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        let mut pool = BufferPool::<Vec<u8>>::new();
        pool.release_vec(vec![0u8; 1024]);
        assert_eq!(pool.len(), 1);

        let v = pool.acquire_vec(1024);
        assert_eq!(v.capacity(), 1024);
        assert!(pool.is_empty());
    }

    #[test]
    fn larger_satisfies_smaller() {
        let mut pool = BufferPool::<Vec<u8>>::new();
        pool.release_vec(vec![0u8; 8192]);
        assert_eq!(pool.len(), 1);

        let v = pool.acquire_vec(1024);
        assert!(v.capacity() >= 1024);
        assert!(pool.is_empty());
    }

    #[test]
    fn best_fit_picks_smallest_that_fits() {
        let mut pool = BufferPool::<Vec<u8>>::new();
        pool.release_vec(vec![0u8; 4096]);
        pool.release_vec(vec![0u8; 8192]);
        pool.release_vec(vec![0u8; 2048]);
        assert_eq!(pool.len(), 3);

        let v = pool.acquire_vec(2000);
        assert_eq!(v.capacity(), 2048);
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn fallback_to_create_on_empty_pool() {
        let mut pool = BufferPool::<Vec<u8>>::new();
        let v = pool.acquire_vec(512);
        assert!(v.capacity() >= 512);
        assert!(pool.is_empty());
    }

    #[test]
    fn fallback_to_create_when_nothing_fits() {
        let mut pool = BufferPool::<Vec<u8>>::new();
        pool.release_vec(vec![0u8; 256]);
        pool.release_vec(vec![0u8; 512]);

        let v = pool.acquire_vec(1024);
        assert!(v.capacity() >= 1024);
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn clear_frees_all() {
        let mut pool = BufferPool::<Vec<u8>>::new();
        pool.release_vec(vec![0u8; 1024]);
        pool.release_vec(vec![0u8; 2048]);
        assert_eq!(pool.len(), 2);
        pool.clear();
        assert!(pool.is_empty());
    }
}
