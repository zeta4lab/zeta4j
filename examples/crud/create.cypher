// CRUD Operations - CREATE Examples
// Basic node and relationship creation patterns
//
// Usage:
//   z4j shell -f examples/crud/create.cypher

// =============================================================================
// CREATE Nodes
// =============================================================================

// Create a single node with label
CREATE (p:Person {name: 'John Doe'});

// Create a node with multiple properties
CREATE (p:Person {
  name: 'Jane Smith',
  age: 28,
  email: 'jane@example.com',
  active: true
});

// Create a node with multiple labels
CREATE (e:Person:Employee:Manager {
  name: 'Bob Wilson',
  department: 'Engineering',
  salary: 85000
});

// Create multiple nodes at once
CREATE (a:Product {name: 'Laptop', price: 999.99}),
       (b:Product {name: 'Mouse', price: 29.99}),
       (c:Product {name: 'Keyboard', price: 79.99});

// Create a node and return it
CREATE (p:Person {name: 'Alice'})
RETURN p;

// Create a node with timestamp
CREATE (e:Event {
  title: 'Meeting',
  created_at: datetime(),
  date: date('2024-01-15')
});

// =============================================================================
// CREATE Relationships
// =============================================================================

// Create a relationship between existing nodes
MATCH (a:Person {name: 'John Doe'}), (b:Person {name: 'Jane Smith'})
CREATE (a)-[:KNOWS]->(b);

// Create a relationship with properties
MATCH (a:Person {name: 'John Doe'}), (b:Person {name: 'Jane Smith'})
CREATE (a)-[:WORKS_WITH {since: 2020, project: 'Alpha'}]->(b);

// Create bidirectional relationships
MATCH (a:Person {name: 'John Doe'}), (b:Person {name: 'Jane Smith'})
CREATE (a)-[:FRIEND]->(b),
       (b)-[:FRIEND]->(a);

// Create node and relationship in one query
CREATE (p:Person {name: 'New Employee'})-[:REPORTS_TO]->(:Person {name: 'Manager X'});

// Create relationship with variable
MATCH (e:Person:Employee {name: 'Bob Wilson'}), (d:Product {name: 'Laptop'})
CREATE (e)-[r:PURCHASED {quantity: 2, date: date()}]->(d)
RETURN type(r), r.quantity;

// =============================================================================
// CREATE with MERGE (Create if not exists)
// =============================================================================

// MERGE creates node only if it doesn't exist
MERGE (p:Person {email: 'unique@example.com'})
ON CREATE SET p.name = 'New User', p.created_at = datetime()
ON MATCH SET p.last_seen = datetime()
RETURN p;

// =============================================================================
// CREATE with FOREACH (Bulk creation)
// =============================================================================

// Create multiple nodes from a list
WITH ['Alice', 'Bob', 'Charlie', 'Diana'] AS names
FOREACH (name IN names |
  CREATE (:TestPerson {name: name})
);

// Create nodes with index
WITH range(1, 10) AS numbers
FOREACH (i IN numbers |
  CREATE (:Counter {value: i})
);

// =============================================================================
// CREATE with UNWIND (Bulk creation from list)
// =============================================================================

// Create nodes from a list of maps
UNWIND [
  {name: 'Tokyo', country: 'Japan'},
  {name: 'Paris', country: 'France'},
  {name: 'London', country: 'UK'}
] AS city
CREATE (c:City {name: city.name, country: city.country});

// Create nodes and relationships from data
UNWIND [
  {from: 'Tokyo', to: 'Paris', distance: 9713},
  {from: 'Paris', to: 'London', distance: 344},
  {from: 'London', to: 'Tokyo', distance: 9571}
] AS route
MATCH (a:City {name: route.from}), (b:City {name: route.to})
CREATE (a)-[:FLIGHT {distance: route.distance}]->(b);

// =============================================================================
// Cleanup test data (optional)
// =============================================================================

// To clean up test data, run:
// MATCH (n:Person) WHERE n.name IN ['John Doe', 'Jane Smith', 'Bob Wilson', 'Alice', 'New Employee', 'Manager X'] DETACH DELETE n;
// MATCH (n:Product) DETACH DELETE n;
// MATCH (n:Event) DETACH DELETE n;
// MATCH (n:TestPerson) DETACH DELETE n;
// MATCH (n:Counter) DETACH DELETE n;
// MATCH (n:City) DETACH DELETE n;
