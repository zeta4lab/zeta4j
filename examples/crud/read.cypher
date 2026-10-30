// CRUD Operations - READ Examples
// Various query patterns for reading data
//
// Usage:
//   z4j shell -f examples/crud/read.cypher
//
// Note: Run create.cypher first to populate test data

// =============================================================================
// Basic READ (MATCH)
// =============================================================================

// Find all nodes of a label
MATCH (p:Person) RETURN p;

// Find specific node by property
MATCH (p:Person {name: 'John Doe'}) RETURN p;

// Find node with WHERE clause
MATCH (p:Person) WHERE p.age > 25 RETURN p.name, p.age;

// Find nodes by multiple conditions
MATCH (p:Person)
WHERE p.age >= 25 AND p.active = true
RETURN p.name;

// Find with OR condition
MATCH (p:Person)
WHERE p.name = 'John Doe' OR p.name = 'Jane Smith'
RETURN p;

// Find with IN clause
MATCH (p:Person)
WHERE p.name IN ['John Doe', 'Jane Smith', 'Bob Wilson']
RETURN p.name;

// =============================================================================
// Pattern Matching
// =============================================================================

// Find related nodes
MATCH (a:Person)-[:KNOWS]->(b:Person)
RETURN a.name AS knows, b.name AS known;

// Find relationship with properties
MATCH (a:Person)-[r:WORKS_WITH]->(b:Person)
RETURN a.name, b.name, r.since, r.project;

// Find bidirectional relationships
MATCH (a:Person)-[:FRIEND]-(b:Person)
WHERE a.name < b.name  // Avoid duplicates
RETURN a.name, b.name;

// Variable length paths
MATCH path = (a:Person)-[:KNOWS*1..3]->(b:Person)
RETURN a.name, b.name, length(path) AS hops;

// Optional match (LEFT JOIN)
MATCH (p:Person)
OPTIONAL MATCH (p)-[:PURCHASED]->(product:Product)
RETURN p.name, collect(product.name) AS purchases;

// =============================================================================
// Aggregation Functions
// =============================================================================

// Count nodes
MATCH (p:Person) RETURN count(p) AS personCount;

// Count by group
MATCH (p:Person)
RETURN p.department, count(p) AS count
ORDER BY count DESC;

// Sum, Avg, Min, Max
MATCH (p:Person:Employee)
RETURN sum(p.salary) AS totalSalary,
       avg(p.salary) AS avgSalary,
       min(p.salary) AS minSalary,
       max(p.salary) AS maxSalary;

// Collect into list
MATCH (p:Person)
RETURN collect(p.name) AS allNames;

// Distinct values
MATCH (p:Person)
RETURN DISTINCT p.department;

// =============================================================================
// Sorting and Limiting
// =============================================================================

// Order by single column
MATCH (p:Person) RETURN p.name ORDER BY p.name;

// Order by multiple columns
MATCH (p:Person)
RETURN p.name, p.age
ORDER BY p.age DESC, p.name ASC;

// Limit results
MATCH (p:Person)
RETURN p.name
ORDER BY p.name
LIMIT 5;

// Skip and Limit (pagination)
MATCH (p:Person)
RETURN p.name
ORDER BY p.name
SKIP 10
LIMIT 10;

// =============================================================================
// Working with Properties
// =============================================================================

// Return specific properties
MATCH (p:Person {name: 'Jane Smith'})
RETURN p.name, p.email, p.age;

// Check if property exists
MATCH (p:Person)
WHERE p.email IS NOT NULL
RETURN p.name, p.email;

// Property comparison
MATCH (p:Person)
WHERE p.age >= 25 AND p.age <= 35
RETURN p.name, p.age;

// String operations
MATCH (p:Person)
WHERE p.name STARTS WITH 'J'
RETURN p.name;

MATCH (p:Person)
WHERE p.name CONTAINS 'Smith'
RETURN p.name;

MATCH (p:Person)
WHERE p.email ENDS WITH '@example.com'
RETURN p.name, p.email;

// All shortest paths
MATCH path = allShortestPaths(
  (a:Person {name: 'John Doe'})-[*]-(b:Person {name: 'Bob Wilson'})
)
RETURN path;

// =============================================================================
// Subqueries
// =============================================================================

// EXISTS subquery
MATCH (p:Person)
WHERE EXISTS {
  MATCH (p)-[:PURCHASED]->(:Product)
}
RETURN p.name AS buyer;

// NOT EXISTS
MATCH (p:Person)
WHERE NOT EXISTS {
  MATCH (p)-[:PURCHASED]->(:Product)
}
RETURN p.name AS nonBuyer;

// =============================================================================
// WITH Clause (Pipelining)
// =============================================================================

// Filter then aggregate
MATCH (p:Person)
WHERE p.age > 25
WITH p
RETURN count(p) AS count;

// Multiple aggregations
MATCH (p:Person)-[:PURCHASED]->(product:Product)
WITH p, count(product) AS purchaseCount, sum(product.price) AS totalSpent
WHERE purchaseCount > 1
RETURN p.name, purchaseCount, totalSpent
ORDER BY totalSpent DESC;

// Transform data
MATCH (p:Person)
WITH p, p.age / 10 * 10 AS ageGroup
RETURN ageGroup, collect(p.name) AS people, count(*) AS count;

// =============================================================================
// UNION Queries
// =============================================================================

// Combine results from multiple queries
MATCH (p:Person {name: 'John Doe'})
RETURN p.name AS name, 'Person' AS type
UNION
MATCH (p:Product {name: 'Laptop'})
RETURN p.name AS name, 'Product' AS type;

// UNION ALL (keep duplicates)
MATCH (a:Person)-[:KNOWS]->(b:Person)
RETURN b.name AS connection
UNION ALL
MATCH (a:Person)-[:WORKS_WITH]->(b:Person)
RETURN b.name AS connection;

// =============================================================================
// Returning Complex Structures
// =============================================================================

// Return relationship details
MATCH (a:Person)-[r]->(b:Person)
RETURN type(r) AS relType, properties(r) AS relProps;

// Return labels
MATCH (n)
RETURN DISTINCT labels(n) AS nodeLabels;

// Return with computed values
MATCH (p:Person)
RETURN p.name,
       p.age,
       CASE
         WHEN p.age < 30 THEN 'Young'
         WHEN p.age < 50 THEN 'Middle'
         ELSE 'Senior'
       END AS ageGroup;
