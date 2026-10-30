// Concurrency Example - Data Setup
// Testing concurrent access patterns

// Ticket booking system
CREATE (e1:Event {id: 'EVT001', name: 'Concert', totalSeats: 100, bookedSeats: 0})
CREATE (e2:Event {id: 'EVT002', name: 'Theater', totalSeats: 50, bookedSeats: 45})
CREATE (e3:Event {id: 'EVT003', name: 'Sports', totalSeats: 1000, bookedSeats: 990});

// Individual seats for seat selection
UNWIND range(1, 10) AS seatNum
CREATE (s:Seat {eventId: 'EVT001', number: seatNum, status: 'available', bookedBy: null});

// Shopping cart simulation
CREATE (cart1:Cart {userId: 'U001', items: [], total: 0})
CREATE (cart2:Cart {userId: 'U002', items: [], total: 0});

// Product stock
CREATE (p1:Stock {productId: 'P001', name: 'Limited Edition', available: 5})
CREATE (p2:Stock {productId: 'P002', name: 'Popular Item', available: 100})
CREATE (p3:Stock {productId: 'P003', name: 'Last One', available: 1});

// Rate limiter tokens
CREATE (rl:RateLimiter {apiKey: 'KEY001', tokens: 10, lastRefill: timestamp()});

// Leaderboard
CREATE (lb1:Score {playerId: 'P001', name: 'Alice', score: 1000})
CREATE (lb2:Score {playerId: 'P002', name: 'Bob', score: 950})
CREATE (lb3:Score {playerId: 'P003', name: 'Charlie', score: 900});

// Job queue
CREATE (j1:Job {id: 'JOB001', status: 'pending', priority: 1, createdAt: timestamp()})
CREATE (j2:Job {id: 'JOB002', status: 'pending', priority: 2, createdAt: timestamp()})
CREATE (j3:Job {id: 'JOB003', status: 'processing', priority: 1, worker: 'W001'});

// Distributed lock
CREATE (lock:DistLock {resource: 'critical_section', holder: null, acquiredAt: null, ttl: 30000});

// Session store
CREATE (sess1:Session {sessionId: 'SESS001', userId: 'U001', data: null, expiresAt: timestamp() + 3600000})
CREATE (sess2:Session {sessionId: 'SESS002', userId: 'U002', data: null, expiresAt: timestamp() + 3600000});
