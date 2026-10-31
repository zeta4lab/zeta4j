// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Transaction lock manager.
//!
//! Lock ownership, FIFO wait queues, and the wait-for graph live under one
//! mutex. Every state transition therefore updates the actual blockers and the
//! deadlock graph atomically.

use parking_lot::Mutex;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

use super::{Result, TransactionError, TransactionId};
use crate::core::{EdgeId, NodeId};

/// Lock manager statistics.
#[derive(Debug, Clone, Default)]
pub struct LockStats {
    /// Number of requests that had to wait or failed immediately.
    pub lock_conflicts: u64,
    /// Number of detected deadlocks.
    pub deadlocks_detected: u64,
    /// Total time spent waiting, in nanoseconds.
    pub total_wait_time_ns: u64,
    /// Number of completed or cancelled waits.
    pub wait_count: u64,
    /// Average wait time, in milliseconds.
    pub avg_wait_time_ms: f64,
}

/// Resource protected by a transaction lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockResource {
    /// One graph node.
    Node(NodeId),
    /// One graph edge.
    Edge(EdgeId),
    /// The whole logical database.
    Database,
}

/// Supported lock modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// Multiple readers may hold the resource.
    Shared,
    /// One writer may hold the resource.
    Exclusive,
}

#[derive(Clone)]
struct LockWaiter {
    tx_id: TransactionId,
    mode: LockMode,
    notify: Arc<Notify>,
}

#[derive(Default)]
struct LockEntry {
    owners: Vec<TransactionId>,
    mode: Option<LockMode>,
    waiters: VecDeque<LockWaiter>,
}

impl LockEntry {
    fn owner_is_compatible(&self, tx_id: TransactionId, requested: LockMode) -> bool {
        self.owners.iter().all(|owner| {
            *owner == tx_id
                || (self.mode == Some(LockMode::Shared) && requested == LockMode::Shared)
        })
    }

    fn waiter_position(&self, tx_id: TransactionId) -> Option<usize> {
        self.waiters.iter().position(|waiter| waiter.tx_id == tx_id)
    }

    fn is_queue_eligible(&self, position: usize, mode: LockMode) -> bool {
        position == 0
            || (mode == LockMode::Shared
                && self
                    .waiters
                    .iter()
                    .take(position)
                    .all(|waiter| waiter.mode == LockMode::Shared))
    }

    fn blockers_for(&self, position: usize, tx_id: TransactionId) -> BTreeSet<TransactionId> {
        let requested = self.waiters[position].mode;
        let mut blockers = BTreeSet::new();

        if requested == LockMode::Exclusive || self.mode == Some(LockMode::Exclusive) {
            blockers.extend(self.owners.iter().copied().filter(|owner| *owner != tx_id));
        }

        for waiter in self.waiters.iter().take(position) {
            if requested == LockMode::Exclusive || waiter.mode == LockMode::Exclusive {
                blockers.insert(waiter.tx_id);
            }
        }

        blockers
    }

    fn blockers_for_new(
        &self,
        tx_id: TransactionId,
        requested: LockMode,
    ) -> BTreeSet<TransactionId> {
        let mut blockers = BTreeSet::new();

        if requested == LockMode::Exclusive || self.mode == Some(LockMode::Exclusive) {
            blockers.extend(self.owners.iter().copied().filter(|owner| *owner != tx_id));
        }

        for waiter in &self.waiters {
            if requested == LockMode::Exclusive || waiter.mode == LockMode::Exclusive {
                blockers.insert(waiter.tx_id);
            }
        }

        blockers
    }

    fn eligible_notifications(&self) -> Vec<Arc<Notify>> {
        let Some(front) = self.waiters.front() else {
            return Vec::new();
        };

        if !self.owner_is_compatible(front.tx_id, front.mode) {
            return Vec::new();
        }

        if front.mode == LockMode::Exclusive {
            return vec![front.notify.clone()];
        }

        self.waiters
            .iter()
            .take_while(|waiter| {
                waiter.mode == LockMode::Shared
                    && self.owner_is_compatible(waiter.tx_id, waiter.mode)
            })
            .map(|waiter| waiter.notify.clone())
            .collect()
    }

    fn is_empty(&self) -> bool {
        self.owners.is_empty() && self.waiters.is_empty()
    }
}

#[derive(Debug, Default)]
struct WaitForGraph {
    edges: HashMap<TransactionId, BTreeSet<TransactionId>>,
}

impl WaitForGraph {
    fn replace_blockers(&mut self, waiter: TransactionId, blockers: BTreeSet<TransactionId>) {
        if blockers.is_empty() {
            self.edges.remove(&waiter);
        } else {
            self.edges.insert(waiter, blockers);
        }
    }

    fn remove_waiter(&mut self, tx_id: TransactionId) {
        self.edges.remove(&tx_id);
    }

    fn remove_transaction(&mut self, tx_id: TransactionId) {
        self.edges.remove(&tx_id);
        for blockers in self.edges.values_mut() {
            blockers.remove(&tx_id);
        }
        self.edges.retain(|_, blockers| !blockers.is_empty());
    }

    fn detect_cycle(&self, start: TransactionId) -> Option<Vec<TransactionId>> {
        let mut visited = HashSet::new();
        let mut active = HashMap::new();
        let mut path = Vec::new();
        self.detect_cycle_from(start, &mut visited, &mut active, &mut path)
    }

    fn detect_cycle_from(
        &self,
        current: TransactionId,
        visited: &mut HashSet<TransactionId>,
        active: &mut HashMap<TransactionId, usize>,
        path: &mut Vec<TransactionId>,
    ) -> Option<Vec<TransactionId>> {
        if let Some(position) = active.get(&current).copied() {
            let mut cycle = path[position..].to_vec();
            cycle.push(current);
            return Some(cycle);
        }
        if !visited.insert(current) {
            return None;
        }

        active.insert(current, path.len());
        path.push(current);
        if let Some(blockers) = self.edges.get(&current) {
            for blocker in blockers {
                if let Some(cycle) = self.detect_cycle_from(*blocker, visited, active, path) {
                    return Some(cycle);
                }
            }
        }
        path.pop();
        active.remove(&current);
        None
    }
}

#[derive(Default)]
struct LockTableState {
    locks: HashMap<LockResource, LockEntry>,
    wait_graph: WaitForGraph,
}

enum AcquireAttempt {
    Acquired,
    Waiting {
        notify: Arc<Notify>,
        newly_registered: bool,
    },
    Deadlock(Vec<TransactionId>),
    Failed(String),
}

/// Two-phase transaction lock manager with FIFO fairness and deadlock detection.
pub struct LockManager {
    state: Mutex<LockTableState>,
    stats_lock_conflicts: AtomicU64,
    stats_deadlocks_detected: AtomicU64,
    stats_total_wait_time_ns: AtomicU64,
    stats_wait_count: AtomicU64,
}

impl Default for LockManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LockManager {
    /// Create an empty lock manager.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(LockTableState::default()),
            stats_lock_conflicts: AtomicU64::new(0),
            stats_deadlocks_detected: AtomicU64::new(0),
            stats_total_wait_time_ns: AtomicU64::new(0),
            stats_wait_count: AtomicU64::new(0),
        }
    }

    /// Try to acquire a lock without waiting.
    pub fn acquire_lock(
        &self,
        tx_id: TransactionId,
        resource: LockResource,
        mode: LockMode,
    ) -> Result<()> {
        let mut state = self.state.lock();
        if Self::try_grant(&mut state, tx_id, resource, mode)? {
            return Ok(());
        }

        let blockers = state
            .locks
            .get(&resource)
            .map(|entry| entry.blockers_for_new(tx_id, mode))
            .unwrap_or_default();
        state.wait_graph.replace_blockers(tx_id, blockers);
        let cycle = state.wait_graph.detect_cycle(tx_id);
        state.wait_graph.remove_waiter(tx_id);
        drop(state);

        self.stats_lock_conflicts.fetch_add(1, Ordering::Relaxed);
        if let Some(cycle) = cycle {
            self.stats_deadlocks_detected
                .fetch_add(1, Ordering::Relaxed);
            Err(TransactionError::DeadlockDetected(cycle))
        } else {
            Err(TransactionError::lock_timeout())
        }
    }

    /// Acquire a lock while waiting up to `timeout_ms`.
    ///
    /// The wait registration is cancellation-safe: dropping this future removes
    /// its queue entry and wait-for edges before waking the next eligible waiter.
    pub async fn acquire_lock_with_timeout(
        &self,
        tx_id: TransactionId,
        resource: LockResource,
        mode: LockMode,
        timeout_ms: u64,
    ) -> Result<()> {
        let started = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        let mut cleanup: Option<WaitRegistration<'_>> = None;

        loop {
            match self.try_acquire_or_enqueue(tx_id, resource, mode) {
                AcquireAttempt::Acquired => {
                    if let Some(mut registration) = cleanup {
                        registration.disarm();
                        self.record_wait(started);
                    }
                    return Ok(());
                }
                AcquireAttempt::Deadlock(cycle) => {
                    self.stats_deadlocks_detected
                        .fetch_add(1, Ordering::Relaxed);
                    if cleanup.is_none() {
                        self.record_wait(started);
                    }
                    return Err(TransactionError::DeadlockDetected(cycle));
                }
                AcquireAttempt::Failed(message) => {
                    return Err(TransactionError::LockFailed(message));
                }
                AcquireAttempt::Waiting {
                    notify,
                    newly_registered,
                } => {
                    if newly_registered {
                        self.stats_lock_conflicts.fetch_add(1, Ordering::Relaxed);
                        cleanup = Some(WaitRegistration::new(self, tx_id, resource, started));
                        tracing::info!(
                            transaction_id = tx_id,
                            resource = ?resource,
                            mode = ?mode,
                            "transaction lock wait registered"
                        );
                    }

                    let remaining = timeout.saturating_sub(started.elapsed());
                    if remaining.is_zero()
                        || tokio::time::timeout(remaining, notify.notified())
                            .await
                            .is_err()
                    {
                        return Err(TransactionError::LockTimeout(timeout_ms));
                    }
                }
            }
        }
    }

    fn try_acquire_or_enqueue(
        &self,
        tx_id: TransactionId,
        resource: LockResource,
        mode: LockMode,
    ) -> AcquireAttempt {
        let mut state = self.state.lock();
        match Self::try_grant(&mut state, tx_id, resource, mode) {
            Ok(true) => return AcquireAttempt::Acquired,
            Err(TransactionError::LockFailed(message)) => {
                return AcquireAttempt::Failed(message);
            }
            Err(error) => return AcquireAttempt::Failed(error.to_string()),
            Ok(false) => {}
        }

        let (notify, newly_registered) = {
            let entry = state.locks.entry(resource).or_default();
            if let Some(position) = entry.waiter_position(tx_id) {
                let waiter = &entry.waiters[position];
                if waiter.mode != mode {
                    return AcquireAttempt::Failed(format!(
                        "transaction {tx_id} already waits for {resource:?} in {:?} mode",
                        waiter.mode
                    ));
                }
                (waiter.notify.clone(), false)
            } else {
                let notify = Arc::new(Notify::new());
                entry.waiters.push_back(LockWaiter {
                    tx_id,
                    mode,
                    notify: notify.clone(),
                });
                (notify, true)
            }
        };

        Self::refresh_resource_waiters(&mut state, resource);
        if let Some(cycle) = state.wait_graph.detect_cycle(tx_id) {
            Self::remove_waiter_locked(&mut state, tx_id, resource);
            let notifications = Self::eligible_notifications(&state, resource);
            drop(state);
            Self::notify_all(notifications);
            return AcquireAttempt::Deadlock(cycle);
        }

        AcquireAttempt::Waiting {
            notify,
            newly_registered,
        }
    }

    fn try_grant(
        state: &mut LockTableState,
        tx_id: TransactionId,
        resource: LockResource,
        mode: LockMode,
    ) -> Result<bool> {
        if let Some(entry) = state.locks.get(&resource)
            && entry.owners.contains(&tx_id)
            && (entry.mode == Some(LockMode::Exclusive)
                || (entry.mode == Some(LockMode::Shared) && mode == LockMode::Shared))
        {
            return Ok(true);
        }

        let can_grant = {
            let entry = state.locks.entry(resource).or_default();

            if entry.owners.contains(&tx_id) {
                if entry.mode == Some(LockMode::Exclusive) || mode == LockMode::Shared {
                    true
                } else {
                    let position = entry.waiter_position(tx_id);
                    let queue_eligible = position
                        .map(|position| entry.is_queue_eligible(position, mode))
                        .unwrap_or(entry.waiters.is_empty());
                    queue_eligible && entry.owner_is_compatible(tx_id, mode)
                }
            } else {
                let position = entry.waiter_position(tx_id);
                let queue_eligible = position
                    .map(|position| entry.is_queue_eligible(position, mode))
                    .unwrap_or(entry.waiters.is_empty());
                queue_eligible && entry.owner_is_compatible(tx_id, mode)
            }
        };

        if !can_grant {
            return Ok(false);
        }

        {
            let entry = state.locks.entry(resource).or_default();
            entry.waiters.retain(|waiter| waiter.tx_id != tx_id);
            if !entry.owners.contains(&tx_id) {
                entry.owners.push(tx_id);
            }
            entry.mode = Some(mode);
        }
        state.wait_graph.remove_waiter(tx_id);
        Self::refresh_resource_waiters(state, resource);
        Ok(true)
    }

    fn refresh_resource_waiters(state: &mut LockTableState, resource: LockResource) {
        let replacements = state
            .locks
            .get(&resource)
            .map(|entry| {
                entry
                    .waiters
                    .iter()
                    .enumerate()
                    .map(|(position, waiter)| {
                        (waiter.tx_id, entry.blockers_for(position, waiter.tx_id))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for (waiter, blockers) in replacements {
            state.wait_graph.replace_blockers(waiter, blockers);
        }
    }

    fn remove_waiter_locked(
        state: &mut LockTableState,
        tx_id: TransactionId,
        resource: LockResource,
    ) {
        if let Some(entry) = state.locks.get_mut(&resource) {
            entry.waiters.retain(|waiter| waiter.tx_id != tx_id);
        }
        state.wait_graph.remove_waiter(tx_id);
        Self::refresh_resource_waiters(state, resource);
        Self::remove_empty_entry(state, resource);
    }

    fn cancel_wait(&self, tx_id: TransactionId, resource: LockResource) {
        let notifications = {
            let mut state = self.state.lock();
            Self::remove_waiter_locked(&mut state, tx_id, resource);
            Self::eligible_notifications(&state, resource)
        };
        Self::notify_all(notifications);
    }

    fn eligible_notifications(state: &LockTableState, resource: LockResource) -> Vec<Arc<Notify>> {
        state
            .locks
            .get(&resource)
            .map(LockEntry::eligible_notifications)
            .unwrap_or_default()
    }

    fn remove_empty_entry(state: &mut LockTableState, resource: LockResource) {
        if state.locks.get(&resource).is_some_and(LockEntry::is_empty) {
            state.locks.remove(&resource);
        }
    }

    fn notify_all(notifications: Vec<Arc<Notify>>) {
        for notify in notifications {
            notify.notify_one();
        }
    }

    fn record_wait(&self, started: Instant) {
        let elapsed = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.stats_total_wait_time_ns
            .fetch_add(elapsed, Ordering::Relaxed);
        self.stats_wait_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Release one lock held by a transaction.
    pub fn release_lock(&self, tx_id: TransactionId, resource: LockResource) {
        let notifications = {
            let mut state = self.state.lock();
            if let Some(entry) = state.locks.get_mut(&resource) {
                entry.owners.retain(|owner| *owner != tx_id);
                if entry.owners.is_empty() {
                    entry.mode = None;
                }
            }
            Self::refresh_resource_waiters(&mut state, resource);
            let notifications = Self::eligible_notifications(&state, resource);
            Self::remove_empty_entry(&mut state, resource);
            notifications
        };
        Self::notify_all(notifications);
    }

    /// Release every held lock and queued request for a transaction.
    pub fn release_all_locks(&self, tx_id: TransactionId) {
        let notifications = {
            let mut state = self.state.lock();
            let affected = state
                .locks
                .iter()
                .filter_map(|(resource, entry)| {
                    (entry.owners.contains(&tx_id)
                        || entry.waiters.iter().any(|waiter| waiter.tx_id == tx_id))
                    .then_some(*resource)
                })
                .collect::<Vec<_>>();

            for resource in &affected {
                if let Some(entry) = state.locks.get_mut(resource) {
                    entry.owners.retain(|owner| *owner != tx_id);
                    entry.waiters.retain(|waiter| waiter.tx_id != tx_id);
                    if entry.owners.is_empty() {
                        entry.mode = None;
                    }
                }
            }
            state.wait_graph.remove_transaction(tx_id);

            let mut notifications = Vec::new();
            for resource in affected {
                Self::refresh_resource_waiters(&mut state, resource);
                notifications.extend(Self::eligible_notifications(&state, resource));
                Self::remove_empty_entry(&mut state, resource);
            }
            notifications
        };
        Self::notify_all(notifications);
    }

    /// Detect a cycle reachable from `tx_id`.
    pub fn detect_deadlock(&self, tx_id: TransactionId) -> Option<Vec<TransactionId>> {
        self.state.lock().wait_graph.detect_cycle(tx_id)
    }

    /// Number of transactions currently represented as waiters.
    pub fn wait_graph_size(&self) -> usize {
        self.state.lock().wait_graph.edges.len()
    }

    /// Number of queued lock requests.
    pub fn waiter_count(&self) -> usize {
        self.state
            .lock()
            .locks
            .values()
            .map(|entry| entry.waiters.len())
            .sum()
    }

    /// Number of locks held by one transaction.
    pub fn locks_held_by(&self, tx_id: TransactionId) -> usize {
        self.state
            .lock()
            .locks
            .values()
            .filter(|entry| entry.owners.contains(&tx_id))
            .count()
    }

    /// Snapshot the current wait-for relationships with their resources.
    pub fn get_wait_graph(&self) -> Vec<(TransactionId, LockResource, Vec<TransactionId>)> {
        let state = self.state.lock();
        let mut result = Vec::new();
        for (resource, entry) in &state.locks {
            for waiter in &entry.waiters {
                let blockers = state
                    .wait_graph
                    .edges
                    .get(&waiter.tx_id)
                    .map(|blockers| blockers.iter().copied().collect())
                    .unwrap_or_default();
                result.push((waiter.tx_id, *resource, blockers));
            }
        }
        result.sort_by_key(|(tx_id, resource, _)| (*tx_id, format!("{resource:?}")));
        result
    }

    /// Summarize the current lock table.
    pub fn lock_summary(&self) -> LockSummary {
        let state = self.state.lock();
        LockSummary {
            total_locks: state.locks.len(),
            exclusive_locks: state
                .locks
                .values()
                .filter(|entry| entry.mode == Some(LockMode::Exclusive))
                .count(),
            shared_locks: state
                .locks
                .values()
                .filter(|entry| entry.mode == Some(LockMode::Shared))
                .count(),
            waiting_transactions: state.wait_graph.edges.len(),
        }
    }

    /// Read lock conflict and wait statistics.
    pub fn get_lock_stats(&self) -> LockStats {
        let lock_conflicts = self.stats_lock_conflicts.load(Ordering::Relaxed);
        let deadlocks_detected = self.stats_deadlocks_detected.load(Ordering::Relaxed);
        let total_wait_time_ns = self.stats_total_wait_time_ns.load(Ordering::Relaxed);
        let wait_count = self.stats_wait_count.load(Ordering::Relaxed);
        let avg_wait_time_ms = if wait_count == 0 {
            0.0
        } else {
            total_wait_time_ns as f64 / wait_count as f64 / 1_000_000.0
        };

        LockStats {
            lock_conflicts,
            deadlocks_detected,
            total_wait_time_ns,
            wait_count,
            avg_wait_time_ms,
        }
    }
}

struct WaitRegistration<'a> {
    manager: &'a LockManager,
    tx_id: TransactionId,
    resource: LockResource,
    started: Instant,
    active: bool,
}

impl<'a> WaitRegistration<'a> {
    fn new(
        manager: &'a LockManager,
        tx_id: TransactionId,
        resource: LockResource,
        started: Instant,
    ) -> Self {
        Self {
            manager,
            tx_id,
            resource,
            started,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for WaitRegistration<'_> {
    fn drop(&mut self) {
        if self.active {
            self.manager.cancel_wait(self.tx_id, self.resource);
            self.manager.record_wait(self.started);
        }
    }
}

/// Aggregate lock-table counts for diagnostics.
#[derive(Debug, Clone)]
pub struct LockSummary {
    /// Number of resource entries.
    pub total_locks: usize,
    /// Number of resources held exclusively.
    pub exclusive_locks: usize,
    /// Number of resources held in shared mode.
    pub shared_locks: usize,
    /// Number of transactions with wait-for edges.
    pub waiting_transactions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use tokio::sync::oneshot;

    async fn wait_until<F>(mut predicate: F) -> std::result::Result<(), String>
    where
        F: FnMut() -> bool,
    {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !predicate() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| "condition was not reached before timeout".to_string())
    }

    async fn bounded<F, T>(future: F) -> std::result::Result<T, String>
    where
        F: Future<Output = T>,
    {
        tokio::time::timeout(Duration::from_secs(2), future)
            .await
            .map_err(|_| "future did not complete before timeout".to_string())
    }

    #[tokio::test]
    async fn release_after_registration_does_not_lose_wakeup() -> std::result::Result<(), String> {
        let manager = Arc::new(LockManager::new());
        let resource = LockResource::Node(1);
        manager
            .acquire_lock(1, resource, LockMode::Exclusive)
            .map_err(|error| error.to_string())?;

        let waiter_manager = manager.clone();
        let waiter = tokio::spawn(async move {
            waiter_manager
                .acquire_lock_with_timeout(2, resource, LockMode::Exclusive, 5_000)
                .await
        });
        wait_until(|| manager.waiter_count() == 1).await?;
        manager.release_all_locks(1);

        bounded(waiter)
            .await?
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        assert_eq!(manager.wait_graph_size(), 0);
        assert_eq!(manager.waiter_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_wait_removes_queue_and_graph() -> std::result::Result<(), String> {
        let manager = Arc::new(LockManager::new());
        let resource = LockResource::Node(2);
        manager
            .acquire_lock(1, resource, LockMode::Exclusive)
            .map_err(|error| error.to_string())?;

        let waiter_manager = manager.clone();
        let waiter = tokio::spawn(async move {
            waiter_manager
                .acquire_lock_with_timeout(2, resource, LockMode::Exclusive, 30_000)
                .await
        });
        wait_until(|| manager.waiter_count() == 1).await?;
        waiter.abort();
        let _ = bounded(waiter).await?;
        wait_until(|| manager.waiter_count() == 0 && manager.wait_graph_size() == 0).await?;
        assert_eq!(manager.locks_held_by(1), 1);
        Ok(())
    }

    #[tokio::test]
    async fn releasing_other_resource_keeps_actual_blocker_edge() -> std::result::Result<(), String>
    {
        let manager = Arc::new(LockManager::new());
        let unrelated = LockResource::Node(30);
        let blocked = LockResource::Node(31);
        manager
            .acquire_lock(1, unrelated, LockMode::Exclusive)
            .map_err(|error| error.to_string())?;
        manager
            .acquire_lock(1, blocked, LockMode::Exclusive)
            .map_err(|error| error.to_string())?;

        let waiter_manager = manager.clone();
        let waiter = tokio::spawn(async move {
            waiter_manager
                .acquire_lock_with_timeout(2, blocked, LockMode::Exclusive, 30_000)
                .await
        });
        wait_until(|| manager.waiter_count() == 1).await?;
        manager.release_lock(1, unrelated);

        assert_eq!(manager.get_wait_graph(), vec![(2, blocked, vec![1])]);
        waiter.abort();
        let _ = bounded(waiter).await?;
        Ok(())
    }

    #[tokio::test]
    async fn shared_holder_release_replaces_blockers() -> std::result::Result<(), String> {
        let manager = Arc::new(LockManager::new());
        let resource = LockResource::Node(32);
        manager
            .acquire_lock(1, resource, LockMode::Shared)
            .map_err(|error| error.to_string())?;
        manager
            .acquire_lock(2, resource, LockMode::Shared)
            .map_err(|error| error.to_string())?;

        let waiter_manager = manager.clone();
        let waiter = tokio::spawn(async move {
            waiter_manager
                .acquire_lock_with_timeout(3, resource, LockMode::Exclusive, 30_000)
                .await
        });
        wait_until(|| manager.waiter_count() == 1).await?;
        assert_eq!(manager.get_wait_graph(), vec![(3, resource, vec![1, 2])]);

        manager.release_lock(1, resource);
        assert_eq!(manager.get_wait_graph(), vec![(3, resource, vec![2])]);
        waiter.abort();
        let _ = bounded(waiter).await?;
        Ok(())
    }

    #[tokio::test]
    async fn unrelated_release_does_not_wake_other_resource() -> std::result::Result<(), String> {
        let manager = Arc::new(LockManager::new());
        let first_resource = LockResource::Node(40);
        let second_resource = LockResource::Node(41);
        manager
            .acquire_lock(1, first_resource, LockMode::Exclusive)
            .map_err(|error| error.to_string())?;
        manager
            .acquire_lock(2, second_resource, LockMode::Exclusive)
            .map_err(|error| error.to_string())?;

        let first_manager = manager.clone();
        let (first_acquired_tx, first_acquired_rx) = oneshot::channel();
        let first = tokio::spawn(async move {
            first_manager
                .acquire_lock_with_timeout(3, first_resource, LockMode::Exclusive, 30_000)
                .await?;
            let _ = first_acquired_tx.send(());
            Result::<()>::Ok(())
        });
        let second_manager = manager.clone();
        let (second_acquired_tx, mut second_acquired_rx) = oneshot::channel();
        let second = tokio::spawn(async move {
            second_manager
                .acquire_lock_with_timeout(4, second_resource, LockMode::Exclusive, 30_000)
                .await?;
            let _ = second_acquired_tx.send(());
            Result::<()>::Ok(())
        });
        wait_until(|| manager.waiter_count() == 2).await?;

        manager.release_lock(1, first_resource);
        bounded(first_acquired_rx)
            .await?
            .map_err(|_| "first notification was dropped".to_string())?;
        assert!(second_acquired_rx.try_recv().is_err());

        second.abort();
        let _ = bounded(second).await?;
        bounded(first)
            .await?
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[tokio::test]
    async fn queued_writer_is_not_bypassed_by_new_reader() -> std::result::Result<(), String> {
        let manager = Arc::new(LockManager::new());
        let resource = LockResource::Node(3);
        manager
            .acquire_lock(1, resource, LockMode::Shared)
            .map_err(|error| error.to_string())?;

        let writer_manager = manager.clone();
        let (writer_acquired_tx, writer_acquired_rx) = oneshot::channel();
        let (release_writer_tx, release_writer_rx) = oneshot::channel();
        let writer = tokio::spawn(async move {
            writer_manager
                .acquire_lock_with_timeout(2, resource, LockMode::Exclusive, 5_000)
                .await?;
            let _ = writer_acquired_tx.send(());
            let _ = release_writer_rx.await;
            writer_manager.release_all_locks(2);
            Result::<()>::Ok(())
        });
        wait_until(|| manager.waiter_count() == 1).await?;

        let reader_manager = manager.clone();
        let (reader_acquired_tx, mut reader_acquired_rx) = oneshot::channel();
        let reader = tokio::spawn(async move {
            reader_manager
                .acquire_lock_with_timeout(3, resource, LockMode::Shared, 5_000)
                .await?;
            let _ = reader_acquired_tx.send(());
            Result::<()>::Ok(())
        });
        wait_until(|| manager.waiter_count() == 2).await?;
        manager.release_all_locks(1);

        bounded(writer_acquired_rx)
            .await?
            .map_err(|_| "writer notification was dropped".to_string())?;
        assert!(reader_acquired_rx.try_recv().is_err());
        release_writer_tx
            .send(())
            .map_err(|_| "writer release receiver was dropped".to_string())?;
        bounded(reader_acquired_rx)
            .await?
            .map_err(|_| "reader notification was dropped".to_string())?;

        bounded(writer)
            .await?
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        bounded(reader)
            .await?
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[tokio::test]
    async fn two_way_deadlock_selects_cycle_closing_request() -> std::result::Result<(), String> {
        let manager = Arc::new(LockManager::new());
        let first = LockResource::Node(10);
        let second = LockResource::Node(11);
        manager
            .acquire_lock(1, first, LockMode::Exclusive)
            .map_err(|error| error.to_string())?;
        manager
            .acquire_lock(2, second, LockMode::Exclusive)
            .map_err(|error| error.to_string())?;

        let waiter_manager = manager.clone();
        let waiter = tokio::spawn(async move {
            waiter_manager
                .acquire_lock_with_timeout(1, second, LockMode::Exclusive, 5_000)
                .await
        });
        wait_until(|| manager.wait_graph_size() == 1).await?;

        let error = manager
            .acquire_lock_with_timeout(2, first, LockMode::Exclusive, 5_000)
            .await
            .err()
            .ok_or_else(|| "cycle-closing request unexpectedly acquired a lock".to_string())?;
        let TransactionError::DeadlockDetected(cycle) = error else {
            return Err(format!("expected deadlock, got {error}"));
        };
        assert_eq!(cycle.first().copied(), Some(2));
        assert_eq!(cycle.last().copied(), Some(2));

        manager.release_all_locks(2);
        bounded(waiter)
            .await?
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        assert_eq!(manager.wait_graph_size(), 0);
        assert_eq!(manager.waiter_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn three_way_deadlock_reports_one_victim_and_cleans_up() -> std::result::Result<(), String>
    {
        let manager = Arc::new(LockManager::new());
        let resources = [
            LockResource::Node(20),
            LockResource::Node(21),
            LockResource::Node(22),
        ];
        for (index, resource) in resources.iter().enumerate() {
            manager
                .acquire_lock((index + 1) as u64, *resource, LockMode::Exclusive)
                .map_err(|error| error.to_string())?;
        }

        let first_manager = manager.clone();
        let first = tokio::spawn(async move {
            first_manager
                .acquire_lock_with_timeout(1, resources[1], LockMode::Exclusive, 5_000)
                .await
        });
        wait_until(|| manager.wait_graph_size() == 1).await?;
        let second_manager = manager.clone();
        let second = tokio::spawn(async move {
            second_manager
                .acquire_lock_with_timeout(2, resources[2], LockMode::Exclusive, 5_000)
                .await
        });
        wait_until(|| manager.wait_graph_size() == 2).await?;

        let error = manager
            .acquire_lock_with_timeout(3, resources[0], LockMode::Exclusive, 5_000)
            .await
            .err()
            .ok_or_else(|| "cycle-closing request unexpectedly acquired a lock".to_string())?;
        assert!(matches!(error, TransactionError::DeadlockDetected(_)));
        manager.release_all_locks(3);
        bounded(second)
            .await?
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        manager.release_all_locks(2);
        bounded(first)
            .await?
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;

        assert_eq!(manager.get_lock_stats().deadlocks_detected, 1);
        assert_eq!(manager.wait_graph_size(), 0);
        assert_eq!(manager.waiter_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn timeout_cleans_wait_state_and_counts_once() -> std::result::Result<(), String> {
        let manager = LockManager::new();
        let resource = LockResource::Edge(1);
        manager
            .acquire_lock(1, resource, LockMode::Exclusive)
            .map_err(|error| error.to_string())?;
        let error = manager
            .acquire_lock_with_timeout(2, resource, LockMode::Exclusive, 1)
            .await
            .err()
            .ok_or_else(|| "waiter unexpectedly acquired a lock".to_string())?;
        assert!(matches!(error, TransactionError::LockTimeout(1)));
        assert_eq!(manager.waiter_count(), 0);
        assert_eq!(manager.wait_graph_size(), 0);
        let stats = manager.get_lock_stats();
        assert_eq!(stats.lock_conflicts, 1);
        assert_eq!(stats.wait_count, 1);
        Ok(())
    }
}
