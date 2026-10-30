// Transactions Example - Query Tests
// Testing transaction isolation and ACID properties

// =============================================================================
// 1. Basic Transaction Operations
// =============================================================================

// 1.1 Simple read
MATCH (a:Account) RETURN a.id, a.owner, a.balance ORDER BY a.id;

// 1.2 Simple update
MATCH (a:Account {id: 'ACC001'}) SET a.balance = a.balance + 100 RETURN a.balance;

// 1.3 Conditional update
MATCH (a:Account {id: 'ACC001'}) WHERE a.balance >= 100 SET a.balance = a.balance - 100 RETURN a.balance;

// =============================================================================
// 2. Atomicity Tests
// =============================================================================

// 2.1 Transfer between accounts (should be atomic)
// TX1: Debit from Alice, Credit to Bob
MATCH (from:Account {id: 'ACC001'}), (to:Account {id: 'ACC002'})
WHERE from.balance >= 200
SET from.balance = from.balance - 200, to.balance = to.balance + 200
RETURN from.balance AS fromBalance, to.balance AS toBalance;

// 2.2 Verify total balance unchanged (conservation)
MATCH (a:Account) RETURN sum(a.balance) AS totalBalance;

// 2.3 Failed transfer (insufficient funds) - should rollback
MATCH (from:Account {id: 'ACC002'}), (to:Account {id: 'ACC001'})
WHERE from.balance >= 10000
SET from.balance = from.balance - 10000, to.balance = to.balance + 10000
RETURN from.balance, to.balance;

// =============================================================================
// 3. Isolation Level Tests
// =============================================================================

// 3.1 Read Committed - should see committed data only
MATCH (a:Account {id: 'ACC001'}) RETURN a.balance;

// 3.2 Repeatable Read - same read in transaction should return same value
MATCH (a:Account {id: 'ACC001'}) RETURN a.balance AS read1;
// (Second read in same transaction should return same value)

// 3.3 Phantom Read test - count should be consistent within transaction
MATCH (a:Account) RETURN count(a) AS accountCount;

// =============================================================================
// 4. Concurrency Scenarios
// =============================================================================

// 4.1 Counter increment (lost update prevention)
MATCH (c:Counter {name: 'visits'}) SET c.value = c.value + 1 RETURN c.value;

// 4.2 Inventory decrement with check
MATCH (i:Inventory {sku: 'PROD001'}) WHERE i.stock > 0 SET i.stock = i.stock - 1 RETURN i.stock;

// 4.3 Out of stock check
MATCH (i:Inventory {sku: 'PROD003'}) WHERE i.stock > 0 SET i.stock = i.stock - 1 RETURN i.stock;

// =============================================================================
// 5. Optimistic Locking Pattern
// =============================================================================

// 5.1 Read document with version
MATCH (d:Document {id: 'DOC001'}) RETURN d.content, d.version;

// 5.2 Update with version check (optimistic lock)
MATCH (d:Document {id: 'DOC001'}) WHERE d.version = 1
SET d.content = 'Updated content', d.version = d.version + 1
RETURN d.content, d.version;

// 5.3 Conflict detection (version mismatch)
MATCH (d:Document {id: 'DOC001'}) WHERE d.version = 1
SET d.content = 'Conflicting update', d.version = d.version + 1
RETURN d.content, d.version;

// =============================================================================
// 6. Sequence Generation
// =============================================================================

// 6.1 Get next sequence value
MATCH (s:Sequence {name: 'order_id'})
SET s.current = s.current + 1
RETURN s.current AS nextOrderId;

// 6.2 Get multiple sequence values
UNWIND range(1, 5) AS i
MATCH (s:Sequence {name: 'order_id'})
SET s.current = s.current + 1
RETURN s.current AS orderId;

// =============================================================================
// 7. Pessimistic Locking Pattern
// =============================================================================

// 7.1 Acquire lock
MATCH (r:Resource {name: 'shared_resource'}) WHERE r.locked = false
SET r.locked = true, r.owner = 'TX1'
RETURN r.locked, r.owner;

// 7.2 Check lock status
MATCH (r:Resource {name: 'shared_resource'}) RETURN r.locked, r.owner;

// 7.3 Release lock
MATCH (r:Resource {name: 'shared_resource'}) WHERE r.owner = 'TX1'
SET r.locked = false, r.owner = null
RETURN r.locked, r.owner;

// =============================================================================
// 8. Multi-Statement Transactions
// =============================================================================

// 8.1 Create order with inventory check
MATCH (i:Inventory {sku: 'PROD001'}) WHERE i.stock >= 5
SET i.stock = i.stock - 5
CREATE (o:Order {sku: 'PROD001', quantity: 5, status: 'pending'})
RETURN i.stock AS remainingStock, o.quantity;

// 8.2 Order with multiple items
MATCH (i1:Inventory {sku: 'PROD001'}), (i2:Inventory {sku: 'PROD002'})
WHERE i1.stock >= 2 AND i2.stock >= 3
SET i1.stock = i1.stock - 2, i2.stock = i2.stock - 3
CREATE (o:Order {skus: ['PROD001', 'PROD002'], quantities: [2, 3], status: 'pending'})
RETURN i1.stock, i2.stock;

// =============================================================================
// 9. Rollback Scenarios
// =============================================================================

// 9.1 Partial failure - constraint violation
// This should fail and rollback if uniqueness is enforced
MATCH (a:Account {id: 'ACC001'})
CREATE (dup:Account {id: 'ACC001', owner: 'Duplicate', balance: 0})
RETURN dup;

// 9.2 Verify no duplicate was created
MATCH (a:Account {id: 'ACC001'}) RETURN count(a) AS count;

// =============================================================================
// 10. Read-Write Conflicts
// =============================================================================

// 10.1 Write skew scenario setup
MATCH (a:Account) WHERE a.owner IN ['Alice', 'Bob']
WITH sum(a.balance) AS total
MATCH (a:Account {owner: 'Alice'})
WHERE total >= 500
SET a.balance = a.balance - 500
RETURN a.balance;

// 10.2 Final state verification
MATCH (a:Account) RETURN a.id, a.owner, a.balance ORDER BY a.id;
