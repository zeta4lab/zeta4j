// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Database-scoped memory admission ledger.
//!
//! The limiter is deliberately reservation based. A successful reservation owns
//! its bytes until the returned [`MemoryReservation`] is dropped or explicitly
//! transferred. Callers never decrement raw counters.

use std::array;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

/// Memory limit configuration.
#[derive(Debug, Clone)]
pub struct MemoryLimiterConfig {
    pub max_memory_bytes: usize,
    pub warning_threshold: f64,
    pub critical_threshold: f64,
    pub auto_cleanup_enabled: bool,
    pub reject_on_limit: bool,
}

impl Default for MemoryLimiterConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 1024 * 1024 * 1024,
            warning_threshold: 0.8,
            critical_threshold: 0.95,
            auto_cleanup_enabled: true,
            reject_on_limit: true,
        }
    }
}

impl MemoryLimiterConfig {
    pub fn with_max_memory_mb(mut self, mb: usize) -> Self {
        self.max_memory_bytes = mb.saturating_mul(1024 * 1024);
        self
    }

    pub fn with_max_memory_gb(mut self, gb: usize) -> Self {
        self.max_memory_bytes = gb.saturating_mul(1024 * 1024 * 1024);
        self
    }

    pub fn with_warning_threshold(mut self, threshold: f64) -> Self {
        self.warning_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn with_critical_threshold(mut self, threshold: f64) -> Self {
        self.critical_threshold = threshold.clamp(0.0, 1.0);
        self
    }
}

/// Stable accounting buckets. Each in-scope allocation belongs to exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum MemoryCategory {
    CommittedGraph = 0,
    TransactionOverlay = 1,
    Index = 2,
    ObjectPool = 3,
    BufferCache = 4,
    Query = 5,
}

impl MemoryCategory {
    pub const ALL: [Self; 6] = [
        Self::CommittedGraph,
        Self::TransactionOverlay,
        Self::Index,
        Self::ObjectPool,
        Self::BufferCache,
        Self::Query,
    ];

    const COUNT: usize = Self::ALL.len();

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CommittedGraph => "committed_graph",
            Self::TransactionOverlay => "transaction_overlay",
            Self::Index => "index",
            Self::ObjectPool => "object_pool",
            Self::BufferCache => "buffer_cache",
            Self::Query => "query",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryStatus {
    #[default]
    Normal,
    Warning,
    Critical,
    Exceeded,
}

impl MemoryStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Normal | Self::Warning)
    }

    pub fn is_critical(&self) -> bool {
        matches!(self, Self::Critical | Self::Exceeded)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryCategoryStats {
    pub category: Option<MemoryCategory>,
    pub current_usage: usize,
    pub peak_usage: usize,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub current_usage: usize,
    pub max_allowed: usize,
    pub peak_usage: usize,
    pub allocation_count: u64,
    pub deallocation_count: u64,
    pub rejected_allocations: u64,
    pub rejected_bytes: usize,
    pub auto_cleanup_count: u64,
    pub cleanup_succeeded_count: u64,
    pub cleanup_reclaimed_bytes: usize,
    pub reconciliation_drift: isize,
    pub categories: Vec<MemoryCategoryStats>,
    pub status: MemoryStatus,
}

impl MemoryStats {
    pub fn usage_ratio(&self) -> f64 {
        if self.max_allowed == 0 {
            return 0.0;
        }
        self.current_usage as f64 / self.max_allowed as f64
    }

    pub fn available(&self) -> usize {
        self.max_allowed.saturating_sub(self.current_usage)
    }

    pub fn category(&self, category: MemoryCategory) -> Option<&MemoryCategoryStats> {
        self.categories
            .iter()
            .find(|stats| stats.category == Some(category))
    }
}

/// Atomic admission ledger. `current_usage` is the admission authority.
pub struct MemoryLimiter {
    config: RwLock<MemoryLimiterConfig>,
    current_usage: AtomicUsize,
    peak_usage: AtomicUsize,
    category_usage: [AtomicUsize; MemoryCategory::COUNT],
    category_peak: [AtomicUsize; MemoryCategory::COUNT],
    allocation_count: AtomicUsize,
    deallocation_count: AtomicUsize,
    rejected_allocations: AtomicUsize,
    rejected_bytes: AtomicUsize,
    auto_cleanup_count: AtomicUsize,
    cleanup_succeeded_count: AtomicUsize,
    cleanup_reclaimed_bytes: AtomicUsize,
    reconciliation_drift_magnitude: AtomicUsize,
    reconciliation_drift_negative: AtomicBool,
    cleanup_in_progress: AtomicBool,
    cleanup_callback: RwLock<Option<Box<dyn Fn() + Send + Sync>>>,
}

struct CleanupFlight<'a> {
    in_progress: &'a AtomicBool,
}

impl Drop for CleanupFlight<'_> {
    fn drop(&mut self) {
        self.in_progress.store(false, Ordering::Release);
    }
}

impl std::fmt::Debug for MemoryLimiter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryLimiter")
            .field("stats", &self.stats())
            .finish()
    }
}

impl MemoryLimiter {
    pub fn new(config: MemoryLimiterConfig) -> Self {
        Self {
            config: RwLock::new(config),
            current_usage: AtomicUsize::new(0),
            peak_usage: AtomicUsize::new(0),
            category_usage: array::from_fn(|_| AtomicUsize::new(0)),
            category_peak: array::from_fn(|_| AtomicUsize::new(0)),
            allocation_count: AtomicUsize::new(0),
            deallocation_count: AtomicUsize::new(0),
            rejected_allocations: AtomicUsize::new(0),
            rejected_bytes: AtomicUsize::new(0),
            auto_cleanup_count: AtomicUsize::new(0),
            cleanup_succeeded_count: AtomicUsize::new(0),
            cleanup_reclaimed_bytes: AtomicUsize::new(0),
            reconciliation_drift_magnitude: AtomicUsize::new(0),
            reconciliation_drift_negative: AtomicBool::new(false),
            cleanup_in_progress: AtomicBool::new(false),
            cleanup_callback: RwLock::new(None),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(MemoryLimiterConfig::default())
    }

    pub fn with_max_mb(mb: usize) -> Self {
        Self::new(MemoryLimiterConfig::default().with_max_memory_mb(mb))
    }

    pub fn with_max_gb(gb: usize) -> Self {
        Self::new(MemoryLimiterConfig::default().with_max_memory_gb(gb))
    }

    pub fn set_cleanup_callback<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut slot = self
            .cleanup_callback
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(Box::new(callback));
    }

    /// Atomically reserves bytes or returns a retryable admission error.
    ///
    /// A failed first attempt runs at most one synchronous, single-flight cleanup
    /// and retries exactly once.
    pub fn try_reserve(
        self: &Arc<Self>,
        category: MemoryCategory,
        bytes: usize,
    ) -> Result<MemoryReservation, MemoryLimitError> {
        if bytes == 0 {
            return Ok(MemoryReservation::new(Arc::clone(self), category, 0));
        }

        if let Some(new_total) = self.try_reserve_once(category, bytes) {
            self.after_reservation(new_total);
            return Ok(MemoryReservation::new(Arc::clone(self), category, bytes));
        }

        let cleanup_enabled = self
            .config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .auto_cleanup_enabled;
        let cleanup_ran = cleanup_enabled && self.trigger_cleanup();
        if cleanup_ran && self.try_reserve_once(category, bytes).is_some() {
            return Ok(MemoryReservation::new(Arc::clone(self), category, bytes));
        }

        self.rejected_allocations.fetch_add(1, Ordering::Relaxed);
        self.rejected_bytes.fetch_add(bytes, Ordering::Relaxed);
        Err(MemoryLimitError::AllocationDenied {
            category,
            requested: bytes,
            available: self.available(),
            limit: self.max_allowed(),
        })
    }

    fn try_reserve_once(&self, category: MemoryCategory, bytes: usize) -> Option<usize> {
        let config = self
            .config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let mut current = self.current_usage.load(Ordering::Acquire);
        loop {
            let new_total = current.checked_add(bytes)?;
            if config.reject_on_limit && new_total > config.max_memory_bytes {
                return None;
            }
            match self.current_usage.compare_exchange_weak(
                current,
                new_total,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let category_total = self.category_usage[category as usize]
                        .fetch_add(bytes, Ordering::AcqRel)
                        + bytes;
                    update_peak(&self.category_peak[category as usize], category_total);
                    update_peak(&self.peak_usage, new_total);
                    self.allocation_count.fetch_add(1, Ordering::Relaxed);
                    return Some(new_total);
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn after_reservation(&self, new_total: usize) {
        let config = self
            .config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if config.auto_cleanup_enabled
            && self.calculate_status_internal(&config, new_total) == MemoryStatus::Critical
        {
            drop(config);
            self.trigger_cleanup();
        }
    }

    fn release(&self, category: MemoryCategory, bytes: usize) {
        if bytes == 0 {
            return;
        }
        atomic_checked_sub(&self.category_usage[category as usize], bytes);
        atomic_checked_sub(&self.current_usage, bytes);
        self.deallocation_count.fetch_add(1, Ordering::Relaxed);
    }

    fn transfer(&self, from: MemoryCategory, to: MemoryCategory, bytes: usize) {
        if bytes == 0 || from == to {
            return;
        }
        atomic_checked_sub(&self.category_usage[from as usize], bytes);
        let total = self.category_usage[to as usize].fetch_add(bytes, Ordering::AcqRel) + bytes;
        update_peak(&self.category_peak[to as usize], total);
    }

    pub fn current_usage(&self) -> usize {
        self.current_usage.load(Ordering::Acquire)
    }

    pub fn max_allowed(&self) -> usize {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .max_memory_bytes
    }

    pub fn available(&self) -> usize {
        self.max_allowed().saturating_sub(self.current_usage())
    }

    pub fn usage_ratio(&self) -> f64 {
        let max = self.max_allowed();
        if max == 0 {
            return 0.0;
        }
        self.current_usage() as f64 / max as f64
    }

    pub fn status(&self) -> MemoryStatus {
        let config = self
            .config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.calculate_status_internal(&config, self.current_usage())
    }

    fn calculate_status_internal(
        &self,
        config: &MemoryLimiterConfig,
        current: usize,
    ) -> MemoryStatus {
        if current > config.max_memory_bytes {
            return MemoryStatus::Exceeded;
        }
        if config.max_memory_bytes == 0 {
            return MemoryStatus::Normal;
        }
        let ratio = current as f64 / config.max_memory_bytes as f64;
        if ratio >= config.critical_threshold {
            MemoryStatus::Critical
        } else if ratio >= config.warning_threshold {
            MemoryStatus::Warning
        } else {
            MemoryStatus::Normal
        }
    }

    pub fn stats(&self) -> MemoryStats {
        let config = self
            .config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (current, category_current) = self.stable_category_snapshot();
        let drift_magnitude = self.reconciliation_drift_magnitude.load(Ordering::Relaxed);
        let drift = if self.reconciliation_drift_negative.load(Ordering::Relaxed) {
            -(drift_magnitude.min(isize::MAX as usize) as isize)
        } else {
            drift_magnitude.min(isize::MAX as usize) as isize
        };
        MemoryStats {
            current_usage: current,
            max_allowed: config.max_memory_bytes,
            peak_usage: self.peak_usage.load(Ordering::Acquire),
            allocation_count: self.allocation_count.load(Ordering::Relaxed) as u64,
            deallocation_count: self.deallocation_count.load(Ordering::Relaxed) as u64,
            rejected_allocations: self.rejected_allocations.load(Ordering::Relaxed) as u64,
            rejected_bytes: self.rejected_bytes.load(Ordering::Relaxed),
            auto_cleanup_count: self.auto_cleanup_count.load(Ordering::Relaxed) as u64,
            cleanup_succeeded_count: self.cleanup_succeeded_count.load(Ordering::Relaxed) as u64,
            cleanup_reclaimed_bytes: self.cleanup_reclaimed_bytes.load(Ordering::Relaxed),
            reconciliation_drift: drift,
            categories: MemoryCategory::ALL
                .iter()
                .enumerate()
                .map(|(index, category)| MemoryCategoryStats {
                    category: Some(*category),
                    current_usage: category_current[index],
                    peak_usage: self.category_peak[*category as usize].load(Ordering::Acquire),
                })
                .collect(),
            status: self.calculate_status_internal(&config, current),
        }
    }

    fn stable_category_snapshot(&self) -> (usize, [usize; MemoryCategory::COUNT]) {
        loop {
            let before = self.current_usage.load(Ordering::Acquire);
            let categories =
                array::from_fn(|index| self.category_usage[index].load(Ordering::Acquire));
            let after = self.current_usage.load(Ordering::Acquire);
            if before == after
                && categories
                    .iter()
                    .copied()
                    .fold(0usize, usize::saturating_add)
                    == after
            {
                return (after, categories);
            }
            std::hint::spin_loop();
        }
    }

    pub fn update_config(&self, new_config: MemoryLimiterConfig) {
        let mut config = self
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *config = new_config;
    }

    pub fn set_max_memory(&self, bytes: usize) {
        let mut config = self
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        config.max_memory_bytes = bytes;
    }

    pub fn available_until(&self, target_ratio: f64) -> usize {
        let target = (self.max_allowed() as f64 * target_ratio.clamp(0.0, 1.0)) as usize;
        target.saturating_sub(self.current_usage())
    }

    /// Records reconciliation drift without mutating the authoritative ledger.
    pub fn record_reconciliation(&self, measured_bytes: usize) -> isize {
        let current = self.current_usage();
        let (negative, magnitude) = if measured_bytes >= current {
            (false, measured_bytes - current)
        } else {
            (true, current - measured_bytes)
        };
        self.reconciliation_drift_negative
            .store(negative, Ordering::Relaxed);
        self.reconciliation_drift_magnitude
            .store(magnitude, Ordering::Relaxed);
        if negative {
            -(magnitude.min(isize::MAX as usize) as isize)
        } else {
            magnitude.min(isize::MAX as usize) as isize
        }
    }

    fn trigger_cleanup(&self) -> bool {
        if self
            .cleanup_in_progress
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        let _flight = CleanupFlight {
            in_progress: &self.cleanup_in_progress,
        };
        let before = self.current_usage();
        let callback_present = {
            let callback = self
                .cleanup_callback
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(callback) = callback.as_ref() {
                callback();
                true
            } else {
                false
            }
        };
        if callback_present {
            self.auto_cleanup_count.fetch_add(1, Ordering::Relaxed);
            let reclaimed = before.saturating_sub(self.current_usage());
            if reclaimed > 0 {
                self.cleanup_succeeded_count.fetch_add(1, Ordering::Relaxed);
                self.cleanup_reclaimed_bytes
                    .fetch_add(reclaimed, Ordering::Relaxed);
            }
        }
        callback_present
    }
}

impl Default for MemoryLimiter {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// RAII ownership token for bytes admitted by a [`MemoryLimiter`].
pub struct MemoryReservation {
    limiter: Arc<MemoryLimiter>,
    category: MemoryCategory,
    bytes: usize,
}

impl std::fmt::Debug for MemoryReservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryReservation")
            .field("category", &self.category)
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl MemoryReservation {
    fn new(limiter: Arc<MemoryLimiter>, category: MemoryCategory, bytes: usize) -> Self {
        Self {
            limiter,
            category,
            bytes,
        }
    }

    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    pub const fn category(&self) -> MemoryCategory {
        self.category
    }

    pub fn resize(&mut self, new_bytes: usize) -> Result<(), MemoryLimitError> {
        if new_bytes > self.bytes {
            let additional = new_bytes - self.bytes;
            let mut additional_reservation = self.limiter.try_reserve(self.category, additional)?;
            self.bytes = new_bytes;
            additional_reservation.bytes = 0;
        } else if new_bytes < self.bytes {
            self.limiter.release(self.category, self.bytes - new_bytes);
            self.bytes = new_bytes;
        }
        Ok(())
    }

    pub fn split(&mut self, bytes: usize) -> Result<Self, MemoryLimitError> {
        if bytes > self.bytes {
            return Err(MemoryLimitError::ReservationUnderflow {
                reserved: self.bytes,
                requested: bytes,
            });
        }
        self.bytes -= bytes;
        Ok(Self::new(Arc::clone(&self.limiter), self.category, bytes))
    }

    pub fn merge(&mut self, mut other: Self) -> Result<(), MemoryLimitError> {
        if !Arc::ptr_eq(&self.limiter, &other.limiter) || self.category != other.category {
            return Err(MemoryLimitError::IncompatibleReservation);
        }
        self.bytes =
            self.bytes
                .checked_add(other.bytes)
                .ok_or(MemoryLimitError::ArithmeticOverflow {
                    requested: other.bytes,
                })?;
        other.bytes = 0;
        Ok(())
    }

    pub fn transfer_to(&mut self, category: MemoryCategory) {
        self.limiter.transfer(self.category, category, self.bytes);
        self.category = category;
    }
}

impl Drop for MemoryReservation {
    fn drop(&mut self) {
        self.limiter.release(self.category, self.bytes);
        self.bytes = 0;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryLimitError {
    AllocationDenied {
        category: MemoryCategory,
        requested: usize,
        available: usize,
        limit: usize,
    },
    ReservationUnderflow {
        reserved: usize,
        requested: usize,
    },
    IncompatibleReservation,
    ArithmeticOverflow {
        requested: usize,
    },
}

impl std::fmt::Display for MemoryLimitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllocationDenied {
                category,
                requested,
                available,
                limit,
            } => write!(
                formatter,
                "memory admission denied for {}: requested {requested} bytes, {available} available (limit {limit})",
                category.as_str()
            ),
            Self::ReservationUnderflow {
                reserved,
                requested,
            } => write!(
                formatter,
                "memory reservation underflow: {requested} requested from {reserved} reserved bytes"
            ),
            Self::IncompatibleReservation => {
                write!(
                    formatter,
                    "memory reservations have different owners or categories"
                )
            }
            Self::ArithmeticOverflow { requested } => {
                write!(
                    formatter,
                    "memory reservation arithmetic overflow for {requested} bytes"
                )
            }
        }
    }
}

impl std::error::Error for MemoryLimitError {}

fn update_peak(peak: &AtomicUsize, value: usize) {
    let mut observed = peak.load(Ordering::Relaxed);
    while value > observed {
        match peak.compare_exchange_weak(observed, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(current) => observed = current,
        }
    }
}

fn atomic_checked_sub(counter: &AtomicUsize, bytes: usize) {
    let result = counter.try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        current.checked_sub(bytes)
    });
    debug_assert!(result.is_ok(), "reservation ledger underflow");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;

    fn limiter(limit: usize) -> Arc<MemoryLimiter> {
        Arc::new(MemoryLimiter::new(MemoryLimiterConfig {
            max_memory_bytes: limit,
            auto_cleanup_enabled: false,
            ..MemoryLimiterConfig::default()
        }))
    }

    #[test]
    fn reservation_drop_restores_total_and_category() -> Result<(), Box<dyn std::error::Error>> {
        let limiter = limiter(1024);
        {
            let reservation = limiter.try_reserve(MemoryCategory::Query, 512)?;
            assert_eq!(reservation.bytes(), 512);
            assert_eq!(limiter.current_usage(), 512);
            assert_eq!(
                limiter
                    .stats()
                    .category(MemoryCategory::Query)
                    .map(|stats| stats.current_usage),
                Some(512)
            );
        }
        assert_eq!(limiter.current_usage(), 0);
        assert!(
            limiter
                .stats()
                .categories
                .iter()
                .all(|stats| stats.current_usage == 0)
        );
        Ok(())
    }

    #[test]
    fn overflow_and_single_request_over_limit_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let limited = limiter(128);
        assert!(limited.try_reserve(MemoryCategory::Query, 129).is_err());
        let unlimited = limiter(usize::MAX);
        let first = unlimited.try_reserve(MemoryCategory::CommittedGraph, usize::MAX)?;
        assert!(unlimited.try_reserve(MemoryCategory::Query, 1).is_err());
        drop(first);
        Ok(())
    }

    #[test]
    fn concurrent_reservations_never_exceed_limit() {
        const WORKERS: usize = 32;
        const BYTES: usize = 64;
        const LIMIT: usize = 8 * BYTES;
        let limiter = limiter(LIMIT);
        let barrier = Arc::new(Barrier::new(WORKERS));
        let mut threads = Vec::new();
        for _ in 0..WORKERS {
            let limiter = Arc::clone(&limiter);
            let barrier = Arc::clone(&barrier);
            threads.push(thread::spawn(move || {
                barrier.wait();
                limiter.try_reserve(MemoryCategory::Query, BYTES).ok()
            }));
        }
        let reservations: Vec<_> = threads
            .into_iter()
            .filter_map(|thread| thread.join().ok().flatten())
            .collect();
        assert!(reservations.len() <= LIMIT / BYTES);
        assert!(limiter.current_usage() <= LIMIT);
        drop(reservations);
        assert_eq!(limiter.current_usage(), 0);
    }

    #[test]
    fn resize_split_merge_and_transfer_preserve_ownership() -> Result<(), Box<dyn std::error::Error>>
    {
        let limiter = limiter(1024);
        let mut reservation = limiter.try_reserve(MemoryCategory::TransactionOverlay, 300)?;
        reservation.resize(400)?;
        let split = reservation.split(100)?;
        assert!(reservation.split(301).is_err());
        assert_eq!(limiter.current_usage(), 400);
        reservation.merge(split)?;
        reservation.transfer_to(MemoryCategory::CommittedGraph);
        assert_eq!(reservation.bytes(), 400);
        assert_eq!(limiter.current_usage(), 400);
        assert_eq!(
            limiter
                .stats()
                .category(MemoryCategory::CommittedGraph)
                .map(|stats| stats.current_usage),
            Some(400)
        );
        drop(reservation);
        assert_eq!(limiter.current_usage(), 0);
        Ok(())
    }

    #[test]
    fn cleanup_panic_releases_single_flight_guard() {
        let limiter = Arc::new(MemoryLimiter::new(MemoryLimiterConfig {
            max_memory_bytes: 1,
            auto_cleanup_enabled: true,
            ..MemoryLimiterConfig::default()
        }));
        limiter.set_cleanup_callback(|| {
            std::panic::resume_unwind(Box::new("cleanup fixture"));
        });

        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe({
            let limiter = Arc::clone(&limiter);
            move || {
                let _result = limiter.try_reserve(MemoryCategory::Query, 2);
            }
        }));

        assert!(panicked.is_err());
        assert!(!limiter.cleanup_in_progress.load(Ordering::Acquire));
        limiter.set_cleanup_callback(|| {});
        assert!(limiter.try_reserve(MemoryCategory::Query, 2).is_err());
    }

    #[test]
    fn disabled_auto_cleanup_does_not_invoke_callback() {
        let limiter = limiter(1);
        let cleanup_calls = Arc::new(AtomicUsize::new(0));
        let observed_calls = Arc::clone(&cleanup_calls);
        limiter.set_cleanup_callback(move || {
            observed_calls.fetch_add(1, Ordering::Relaxed);
        });

        assert!(limiter.try_reserve(MemoryCategory::Query, 2).is_err());
        assert_eq!(cleanup_calls.load(Ordering::Relaxed), 0);
        assert_eq!(limiter.stats().auto_cleanup_count, 0);
    }
}
