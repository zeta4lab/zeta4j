// CRUD Operations - UPDATE Examples
// Patterns for updating nodes and relationships
//
// Usage:
//   z4j shell -f examples/crud/update.cypher
//
// Note: Run create.cypher first to populate test data

// =============================================================================
// SET - Update Properties
// =============================================================================

// Set a single property
MATCH (p:Person {name: 'John Doe'})
SET p.age = 35
RETURN p;

// Set multiple properties
MATCH (p:Person {name: 'Jane Smith'})
SET p.age = 30, p.city = 'Seoul', p.updated_at = datetime()
RETURN p;

// Set property using +=  (merge properties)
MATCH (p:Person {name: 'John Doe'})
SET p += {department: 'Engineering', level: 'Senior'}
RETURN p;

// Set property from another property
MATCH (p:Person {name: 'Bob Wilson'})
SET p.display_name = p.name + ' (' + p.department + ')'
RETURN p.display_name;

// Set property from another node
MATCH (manager:Person {name: 'Bob Wilson'}), (employee:Person {name: 'John Doe'})
SET employee.manager_id = id(manager)
RETURN employee;

// Conditional update
MATCH (p:Person)
WHERE p.age > 30
SET p.category = 'Senior'
RETURN p.name, p.category;

// =============================================================================
// Update with CASE Expression
// =============================================================================

// Categorize based on conditions
MATCH (p:Person)
SET p.status = CASE
  WHEN p.age < 25 THEN 'Junior'
  WHEN p.age < 35 THEN 'Mid'
  WHEN p.age < 50 THEN 'Senior'
  ELSE 'Executive'
END
RETURN p.name, p.age, p.status;

// Increment values
MATCH (p:Person {name: 'John Doe'})
SET p.login_count = COALESCE(p.login_count, 0) + 1
RETURN p.login_count;

// =============================================================================
// Update Labels
// =============================================================================

// Add a label
MATCH (p:Person {name: 'John Doe'})
SET p:Premium
RETURN labels(p);

// Add multiple labels
MATCH (p:Person {name: 'Jane Smith'})
SET p:VIP:Verified
RETURN labels(p);

// Remove a label
MATCH (p:Person:Premium {name: 'John Doe'})
REMOVE p:Premium
RETURN labels(p);

// =============================================================================
// Update Relationships
// =============================================================================

// =============================================================================
// REMOVE - Delete Properties
// =============================================================================

// Remove a single property
MATCH (p:Person {name: 'John Doe'})
REMOVE p.temporary_field
RETURN p;

// Remove multiple properties
MATCH (p:Person {name: 'Jane Smith'})
REMOVE p.old_field, p.deprecated_field
RETURN p;

// Set property to null (same as remove)
MATCH (p:Person {name: 'Bob Wilson'})
SET p.temp = null
RETURN p;

// =============================================================================
// Bulk Updates
// =============================================================================

// Update all nodes of a label
MATCH (p:Person)
SET p.schema_version = 2
RETURN count(p) AS updated;

// Update with FOREACH
MATCH (p:Person)
WHERE p.department = 'Engineering'
WITH collect(p) AS engineers
FOREACH (e IN engineers |
  SET e.bonus = 5000
);

// Update using UNWIND
UNWIND [
  {name: 'John Doe', newSalary: 90000},
  {name: 'Jane Smith', newSalary: 85000}
] AS update
MATCH (p:Person {name: update.name})
SET p.salary = update.newSalary
RETURN p.name, p.salary;

// =============================================================================
// MERGE for Upsert
// =============================================================================

// Create or update
MERGE (p:Person {email: 'john@example.com'})
ON CREATE SET p.name = 'John', p.created_at = datetime()
ON MATCH SET p.last_login = datetime(), p.login_count = COALESCE(p.login_count, 0) + 1
RETURN p;

// =============================================================================
// Complex Updates
// =============================================================================

// Update based on aggregation
MATCH (p:Person)-[r:PURCHASED]->(product:Product)
WITH p, count(r) AS purchaseCount, sum(product.price) AS totalSpent
SET p.purchase_count = purchaseCount,
    p.total_spent = totalSpent,
    p.customer_tier = CASE
      WHEN totalSpent > 1000 THEN 'Gold'
      WHEN totalSpent > 500 THEN 'Silver'
      ELSE 'Bronze'
    END
RETURN p.name, p.customer_tier;

// Propagate updates through graph
MATCH (manager:Person:Manager)-[:MANAGES*]->(employee:Person)
SET employee.division = manager.division
RETURN employee.name, employee.division;

// =============================================================================
// Replace vs Merge Properties
// =============================================================================

// Replace all properties (dangerous!)
// MATCH (p:Person {name: 'John Doe'})
// SET p = {name: 'John Doe', age: 35}  // Removes all other properties!
// RETURN p;

// Safer: Merge properties
MATCH (p:Person {name: 'John Doe'})
SET p += {age: 35, city: 'Busan'}  // Keeps existing properties
RETURN p;

// =============================================================================
// Atomic Updates
// =============================================================================

// Increment counter atomically
MATCH (counter:Counter {name: 'visits'})
SET counter.value = counter.value + 1
RETURN counter.value;

// Toggle boolean
MATCH (p:Person {name: 'John Doe'})
SET p.active = NOT COALESCE(p.active, false)
RETURN p.active;
