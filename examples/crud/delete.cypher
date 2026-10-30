// CRUD Operations - DELETE Examples
// Patterns for deleting nodes and relationships
//
// Usage:
//   z4j shell -f examples/crud/delete.cypher
//
// WARNING: These operations are destructive!
// Run create.cypher to restore test data after running these examples.

// =============================================================================
// DELETE Relationships
// =============================================================================

// Delete a specific relationship
MATCH (a:Person {name: 'John Doe'})-[r:KNOWS]->(b:Person {name: 'Jane Smith'})
DELETE r
RETURN a.name, b.name;

// Delete relationship by type
MATCH ()-[r:TEMPORARY]->()
DELETE r
RETURN count(r) AS deleted;

// Delete relationships with condition
MATCH (a:Person)-[r:WORKS_WITH]->(b:Person)
WHERE r.ended = true
DELETE r
RETURN count(r) AS deleted;

// Delete all relationships of a node
MATCH (p:Person {name: 'John Doe'})-[r]-()
DELETE r
RETURN count(r) AS deleted;

// Delete specific direction relationships
MATCH (p:Person {name: 'John Doe'})-[r]->()
DELETE r
RETURN count(r) AS deletedOutgoing;

// =============================================================================
// DELETE Nodes
// =============================================================================

// Delete a node (must have no relationships)
MATCH (p:Person {name: 'Temporary User'})
DELETE p;

// Delete node with DETACH (removes relationships too)
MATCH (p:Person {name: 'John Doe'})
DETACH DELETE p;

// Delete multiple nodes
MATCH (p:Person)
WHERE p.status = 'inactive'
DETACH DELETE p
RETURN count(p) AS deleted;

// Delete nodes by list
MATCH (p:Person)
WHERE p.name IN ['Test1', 'Test2', 'Test3']
DETACH DELETE p;

// =============================================================================
// Conditional DELETE
// =============================================================================

// Delete if exists
MATCH (p:Person {email: 'delete@example.com'})
DETACH DELETE p;

// Delete orphan nodes (no relationships)
MATCH (n)
WHERE NOT EXISTS { MATCH (n)-[]-() }
DELETE n
RETURN count(n) AS orphansDeleted;

// =============================================================================
// Safe DELETE Patterns
// =============================================================================

// Soft delete (mark as deleted instead of removing)
MATCH (p:Person {name: 'To Soft Delete'})
SET p.deleted = true, p.deleted_at = datetime()
RETURN p;

// =============================================================================
// Batch DELETE
// =============================================================================

// Delete in batches (for large datasets)
CALL {
  MATCH (p:Person {status: 'spam'})
  WITH p LIMIT 1000
  DETACH DELETE p
  RETURN count(*) AS deleted
}
RETURN deleted;

// Delete with FOREACH
MATCH (p:Person)
WHERE p.role = 'temp'
WITH collect(p) AS toDelete
FOREACH (node IN toDelete |
  DETACH DELETE node
);

// =============================================================================
// Cascade DELETE
// =============================================================================

// Delete node and all related nodes
MATCH (p:Person {name: 'Project Owner'})-[:OWNS]->(project:Project)
MATCH (project)-[:HAS]->(task:Task)
DETACH DELETE task
DETACH DELETE project
DETACH DELETE p;

// Delete tree structure
MATCH (root:TreeNode {id: 1})-[:CHILD*0..]->(descendant:TreeNode)
DETACH DELETE descendant;

// Delete graph component
MATCH (start:Node {name: 'Cluster1'})-[*]-(connected:Node)
DETACH DELETE connected;

// =============================================================================
// Cleanup Queries
// =============================================================================

// Delete all nodes of a label
MATCH (n:TestData)
DETACH DELETE n;

// Delete all relationships of a type
MATCH ()-[r:TEST_REL]->()
DELETE r;

// Delete everything (DANGEROUS!)
// MATCH (n) DETACH DELETE n;

// Delete and count
MATCH (p:Person {department: 'Closed'})
WITH p, count(*) AS total
DETACH DELETE p
RETURN total AS deleted;

// =============================================================================
// Error Handling Examples
// =============================================================================

// This will fail if node has relationships:
// MATCH (p:Person {name: 'Has Relationships'})
// DELETE p;  // Error: Cannot delete node with relationships

// Use DETACH DELETE instead:
MATCH (p:Person {name: 'Has Relationships'})
DETACH DELETE p;

// Or delete relationships first:
MATCH (p:Person {name: 'Has Relationships'})-[r]-()
DELETE r;
MATCH (p:Person {name: 'Has Relationships'})
DELETE p;

// =============================================================================
// Recreate Test Data
// =============================================================================

// After running delete examples, recreate data with:
// Run: z4j shell -f examples/crud/create.cypher
