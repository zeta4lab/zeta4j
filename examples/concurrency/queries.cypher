// Concurrency Example - Query Tests
// Testing concurrent access patterns

// =============================================================================
// 1. Ticket Booking (Race Condition Prevention)
// =============================================================================

// 1.1 Check available seats
MATCH (e:Event {id: 'EVT001'}) RETURN e.name, e.totalSeats - e.bookedSeats AS available;

// 1.2 Book seat with atomic check-and-update
MATCH (e:Event {id: 'EVT001'})
WHERE e.bookedSeats < e.totalSeats
SET e.bookedSeats = e.bookedSeats + 1
RETURN e.bookedSeats AS newBookedCount;

// 1.3 Book multiple seats atomically
MATCH (e:Event {id: 'EVT001'})
WHERE e.bookedSeats + 5 <= e.totalSeats
SET e.bookedSeats = e.bookedSeats + 5
RETURN e.bookedSeats;

// 1.4 Attempt overbooking (should fail gracefully)
MATCH (e:Event {id: 'EVT002'})
WHERE e.bookedSeats + 10 <= e.totalSeats
SET e.bookedSeats = e.bookedSeats + 10
RETURN e.bookedSeats;

// =============================================================================
// 2. Seat Selection (Individual Seat Locking)
// =============================================================================

// 2.1 Find available seats
MATCH (s:Seat {eventId: 'EVT001', status: 'available'})
RETURN s.number ORDER BY s.number;

// 2.2 Reserve specific seat
MATCH (s:Seat {eventId: 'EVT001', number: 1, status: 'available'})
SET s.status = 'reserved', s.bookedBy = 'USER001'
RETURN s.number, s.status, s.bookedBy;

// 2.3 Attempt double booking (should fail)
MATCH (s:Seat {eventId: 'EVT001', number: 1, status: 'available'})
SET s.status = 'reserved', s.bookedBy = 'USER002'
RETURN s.number, s.bookedBy;

// 2.4 Confirm reservation
MATCH (s:Seat {eventId: 'EVT001', number: 1, bookedBy: 'USER001'})
SET s.status = 'confirmed'
RETURN s.number, s.status;

// =============================================================================
// 3. Stock Management (Inventory Race Conditions)
// =============================================================================

// 3.1 Check stock
MATCH (s:Stock) RETURN s.productId, s.name, s.available ORDER BY s.productId;

// 3.2 Purchase with stock check
MATCH (s:Stock {productId: 'P003'})
WHERE s.available >= 1
SET s.available = s.available - 1
RETURN s.productId, s.available;

// 3.3 Attempt purchase of out-of-stock item
MATCH (s:Stock {productId: 'P003'})
WHERE s.available >= 1
SET s.available = s.available - 1
RETURN s.productId, s.available;

// 3.4 Bulk purchase
MATCH (s:Stock {productId: 'P002'})
WHERE s.available >= 10
SET s.available = s.available - 10
RETURN s.productId, s.available;

// =============================================================================
// 4. Rate Limiting
// =============================================================================

// 4.1 Check tokens
MATCH (rl:RateLimiter {apiKey: 'KEY001'}) RETURN rl.tokens;

// 4.2 Consume token
MATCH (rl:RateLimiter {apiKey: 'KEY001'})
WHERE rl.tokens > 0
SET rl.tokens = rl.tokens - 1
RETURN rl.tokens AS remaining;

// 4.3 Refill tokens (periodic)
MATCH (rl:RateLimiter {apiKey: 'KEY001'})
SET rl.tokens = 10, rl.lastRefill = timestamp()
RETURN rl.tokens;

// =============================================================================
// 5. Leaderboard Updates
// =============================================================================

// 5.1 Get current leaderboard
MATCH (s:Score) RETURN s.name, s.score ORDER BY s.score DESC;

// 5.2 Update score (atomic increment)
MATCH (s:Score {playerId: 'P001'})
SET s.score = s.score + 50
RETURN s.name, s.score;

// 5.3 Conditional score update (only if higher)
MATCH (s:Score {playerId: 'P002'})
WHERE 1000 > s.score
SET s.score = 1000
RETURN s.name, s.score;

// =============================================================================
// 6. Job Queue (Worker Coordination)
// =============================================================================

// 6.1 Get pending jobs
MATCH (j:Job {status: 'pending'})
RETURN j.id, j.priority, j.createdAt
ORDER BY j.priority, j.createdAt
LIMIT 5;

// 6.2 Claim job (atomic status change)
MATCH (j:Job {status: 'pending'})
WITH j ORDER BY j.priority, j.createdAt LIMIT 1
SET j.status = 'processing', j.worker = 'WORKER001'
RETURN j.id, j.status, j.worker;

// 6.3 Complete job
MATCH (j:Job {id: 'JOB003', worker: 'W001'})
SET j.status = 'completed', j.completedAt = timestamp()
RETURN j.id, j.status;

// 6.4 Retry failed job
MATCH (j:Job {status: 'failed'})
SET j.status = 'pending', j.worker = null, j.retryCount = coalesce(j.retryCount, 0) + 1
RETURN j.id, j.retryCount;

// =============================================================================
// 7. Distributed Lock
// =============================================================================

// 7.1 Acquire lock
MATCH (lock:DistLock {resource: 'critical_section'})
WHERE lock.holder IS NULL
SET lock.holder = 'PROCESS001', lock.acquiredAt = timestamp()
RETURN lock.resource, lock.holder;

// 7.2 Check lock status
MATCH (lock:DistLock {resource: 'critical_section'})
RETURN lock.holder, lock.acquiredAt;

// 7.3 Attempt lock (should fail if held)
MATCH (lock:DistLock {resource: 'critical_section'})
WHERE lock.holder IS NULL
SET lock.holder = 'PROCESS002', lock.acquiredAt = timestamp()
RETURN lock.holder;

// 7.4 Release lock
MATCH (lock:DistLock {resource: 'critical_section', holder: 'PROCESS001'})
SET lock.holder = null, lock.acquiredAt = null
RETURN lock.resource, lock.holder;

// 7.5 Force release expired lock
MATCH (lock:DistLock {resource: 'critical_section'})
WHERE lock.acquiredAt IS NOT NULL AND timestamp() - lock.acquiredAt > lock.ttl
SET lock.holder = null, lock.acquiredAt = null
RETURN lock.resource, 'expired lock released' AS status;

// =============================================================================
// 8. Session Management
// =============================================================================

// 8.1 Get session
MATCH (s:Session {sessionId: 'SESS001'})
RETURN s.userId, s.data, s.expiresAt;

// 8.2 Update session data
MATCH (s:Session {sessionId: 'SESS001'})
SET s.data = {lastAccess: timestamp(), views: 5}
RETURN s.sessionId, s.data;

// 8.3 Extend session
MATCH (s:Session {sessionId: 'SESS001'})
SET s.expiresAt = timestamp() + 3600000
RETURN s.sessionId, s.expiresAt;

// 8.4 Delete expired sessions
MATCH (s:Session)
WHERE s.expiresAt < timestamp()
DELETE s
RETURN count(*) AS deletedCount;

// =============================================================================
// 9. Counter with Compare-and-Swap Pattern
// =============================================================================

// 9.1 Read counter value
MATCH (e:Event {id: 'EVT001'}) RETURN e.bookedSeats AS current;

// 9.2 CAS-style update (only if value matches expected)
MATCH (e:Event {id: 'EVT001'})
WHERE e.bookedSeats = 10  // Expected value
SET e.bookedSeats = 11    // New value
RETURN e.bookedSeats;

// =============================================================================
// 10. Batch Operations with Concurrency Control
// =============================================================================

// 10.1 Bulk reserve available seats
MATCH (s:Seat {eventId: 'EVT001', status: 'available'})
WITH s LIMIT 3
SET s.status = 'reserved', s.bookedBy = 'BULK_BUYER'
RETURN collect(s.number) AS reservedSeats;

// 10.2 Cancel all reservations for user
MATCH (s:Seat {bookedBy: 'BULK_BUYER', status: 'reserved'})
SET s.status = 'available', s.bookedBy = null
RETURN count(s) AS cancelledCount;
