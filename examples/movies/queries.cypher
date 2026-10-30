// Movie Graph Query Examples
// Run after loading data.cypher
//
// Usage:
//   z4j shell -f examples/movies/queries.cypher

// =============================================================================
// Basic Queries
// =============================================================================

// Find all movies
MATCH (m:Movie) RETURN m.title, m.released ORDER BY m.released;

// Find all persons
MATCH (p:Person) RETURN p.name, p.born ORDER BY p.name;

// Find movies released after 2000
MATCH (m:Movie) WHERE m.released > 2000 RETURN m.title, m.released;

// Find persons born before 1970
MATCH (p:Person) WHERE p.born < 1970 RETURN p.name, p.born ORDER BY p.born;

// =============================================================================
// Pattern Matching Queries
// =============================================================================

// Find who acted in The Matrix
MATCH (p:Person)-[:ACTED_IN]->(m:Movie {title: 'The Matrix'})
RETURN p.name AS actor;

// Find all movies that Tom Hanks acted in
MATCH (tom:Person {name: 'Tom Hanks'})-[:ACTED_IN]->(m:Movie)
RETURN m.title, m.released;

// Find directors and their movies
MATCH (d:Person)-[:DIRECTED]->(m:Movie)
RETURN d.name AS director, m.title AS movie
ORDER BY d.name;

// Find actors and their roles
MATCH (p:Person)-[r:ACTED_IN]->(m:Movie)
RETURN p.name AS actor, m.title AS movie, r.roles AS roles;

// =============================================================================
// Multi-hop Queries
// =============================================================================

// Find co-actors (actors who acted together)
MATCH (p1:Person)-[:ACTED_IN]->(m:Movie)<-[:ACTED_IN]-(p2:Person)
WHERE p1.name < p2.name
RETURN p1.name, p2.name, m.title;

// Find actors who have worked with Tom Hanks
MATCH (tom:Person {name: 'Tom Hanks'})-[:ACTED_IN]->(m:Movie)<-[:ACTED_IN]-(coactor:Person)
RETURN DISTINCT coactor.name AS coactor;

// Find directors who also acted
MATCH (p:Person)-[:DIRECTED]->(m:Movie)
WHERE EXISTS { MATCH (p)-[:ACTED_IN]->(:Movie) }
RETURN p.name;

// =============================================================================
// Aggregation Queries
// =============================================================================

// Count movies per year
MATCH (m:Movie)
RETURN m.released AS year, count(*) AS count
ORDER BY year;

// Find actors with most movies
MATCH (p:Person)-[:ACTED_IN]->(m:Movie)
RETURN p.name, count(m) AS movieCount
ORDER BY movieCount DESC
LIMIT 5;

// Find average release year by director
MATCH (d:Person)-[:DIRECTED]->(m:Movie)
RETURN d.name AS director, avg(m.released) AS avgYear, count(m) AS movieCount
ORDER BY movieCount DESC;

// =============================================================================
// Path Queries
// =============================================================================

// Find shortest path between two actors via movies
MATCH path = shortestPath(
  (keanu:Person {name: 'Keanu Reeves'})-[*]-(tom:Person {name: 'Tom Hanks'})
)
RETURN path;

// Find all paths between Kevin Bacon and other actors (max 4 hops)
MATCH path = (kevin:Person {name: 'Kevin Bacon'})-[*..4]-(other:Person)
WHERE kevin <> other
RETURN other.name, length(path) AS distance
ORDER BY distance;

// =============================================================================
// Complex Queries
// =============================================================================

// Find movies where director also acted
MATCH (p:Person)-[:DIRECTED]->(m:Movie)<-[:ACTED_IN]-(p)
RETURN p.name, m.title;

// Find prolific director-actor pairs
MATCH (d:Person)-[:DIRECTED]->(m:Movie)<-[:ACTED_IN]-(a:Person)
RETURN d.name AS director, a.name AS actor, count(m) AS collaborations
ORDER BY collaborations DESC
LIMIT 10;

// Find movies with most cast members
MATCH (m:Movie)<-[:ACTED_IN]-(p:Person)
RETURN m.title, count(p) AS castSize
ORDER BY castSize DESC
LIMIT 5;

// Find the "Kevin Bacon number" for all actors
MATCH path = shortestPath(
  (kevin:Person {name: 'Kevin Bacon'})-[:ACTED_IN*]-(other:Person)
)
WHERE kevin <> other
RETURN other.name, length(path)/2 AS baconNumber
ORDER BY baconNumber;

// =============================================================================
// Optional Match and Null Handling
// =============================================================================

// Find all persons and their directed movies (if any)
MATCH (p:Person)
OPTIONAL MATCH (p)-[:DIRECTED]->(m:Movie)
RETURN p.name, collect(m.title) AS directedMovies;

// Find movies without a director
MATCH (m:Movie)
WHERE NOT EXISTS { MATCH (:Person)-[:DIRECTED]->(m) }
RETURN m.title;

// =============================================================================
// WITH Clause for Pipeline Processing
// =============================================================================

// Find top 3 directors and their movies
MATCH (d:Person)-[:DIRECTED]->(m:Movie)
WITH d, count(m) AS movieCount
ORDER BY movieCount DESC
LIMIT 3
MATCH (d)-[:DIRECTED]->(m:Movie)
RETURN d.name, collect(m.title) AS movies, movieCount;

// Find actors who acted in movies from different decades
MATCH (p:Person)-[:ACTED_IN]->(m:Movie)
WITH p, collect(DISTINCT m.released / 10 * 10) AS decades
WHERE size(decades) > 1
RETURN p.name, decades
ORDER BY size(decades) DESC;
