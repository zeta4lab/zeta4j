// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! LRU (Least Recently Used) replacement policy.
//!
//! The buffer-pool manager serializes access to this type with a mutex. The
//! implementation delegates its intrusive-list bookkeeping to the safe `lru`
//! crate instead of maintaining raw pointers locally.

use ::lru::LruCache;

/// Frame identifier within the buffer pool.
pub type FrameId = usize;

/// Tracks unpinned frames from least to most recently used.
pub struct LRUReplacer {
    capacity: usize,
    candidates: LruCache<FrameId, ()>,
}

impl LRUReplacer {
    /// Creates an empty replacement set with the given maximum capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            candidates: LruCache::unbounded(),
        }
    }

    /// Atomically selects and removes the least recently used candidate.
    pub fn victim(&mut self) -> Option<FrameId> {
        self.candidates.pop_lru().map(|(frame_id, ())| frame_id)
    }

    /// Alias for [`Self::victim`] that emphasizes eviction intent.
    #[inline]
    pub fn evict_one(&mut self) -> Option<FrameId> {
        self.victim()
    }

    /// Evicts at most `count` candidates, ordered from least to most recent.
    pub fn evict_batch(&mut self, count: usize) -> Vec<FrameId> {
        std::iter::from_fn(|| self.victim()).take(count).collect()
    }

    /// Removes an in-use frame from the replacement candidates.
    pub fn pin(&mut self, frame_id: FrameId) {
        self.candidates.pop(&frame_id);
    }

    /// Adds a frame as the most recently used candidate when capacity permits.
    /// An existing candidate keeps its current position.
    pub fn unpin(&mut self, frame_id: FrameId) {
        if self.candidates.contains(&frame_id) || self.candidates.len() >= self.capacity {
            return;
        }
        self.candidates.put(frame_id, ());
    }

    /// Marks an existing candidate as most recently used.
    pub fn access(&mut self, frame_id: FrameId) {
        self.candidates.promote(&frame_id);
    }

    /// Returns the number of eviction candidates.
    pub fn size(&self) -> usize {
        self.candidates.len()
    }
}

#[cfg(test)]
mod tests {
    use super::LRUReplacer;

    #[test]
    fn evicts_in_lru_order() {
        let mut replacer = LRUReplacer::new(3);
        replacer.unpin(1);
        replacer.unpin(2);
        replacer.unpin(3);
        replacer.access(1);

        assert_eq!(replacer.evict_batch(3), vec![2, 3, 1]);
    }

    #[test]
    fn pin_and_capacity_control_candidates() {
        let mut replacer = LRUReplacer::new(2);
        replacer.unpin(1);
        replacer.unpin(2);
        replacer.unpin(3);
        replacer.pin(1);
        replacer.unpin(3);

        assert_eq!(replacer.size(), 2);
        assert_eq!(replacer.evict_batch(3), vec![2, 3]);
    }

    #[test]
    fn zero_capacity_never_tracks_candidates() {
        let mut replacer = LRUReplacer::new(0);
        replacer.unpin(1);

        assert_eq!(replacer.victim(), None);
    }
}
