// Performance Example - Query Tests
// Testing query performance with large data

// =============================================================================
// 1. Basic Counting and Statistics
// =============================================================================

// 1.1 Count all nodes by label
MATCH (u:User) RETURN count(u) AS userCount;

MATCH (p:Product) RETURN count(p) AS productCount;

// 1.2 Count all relationships
MATCH ()-[r:FRIEND]->() RETURN count(r) AS friendshipCount;

MATCH ()-[r:PURCHASED]->() RETURN count(r) AS purchaseCount;

// =============================================================================
// 2. Index Scan Performance
// =============================================================================

// 2.1 Exact match lookup
MATCH (u:User {id: 'U500'}) RETURN u.name, u.email;

// 2.2 Multiple exact matches
MATCH (u:User) WHERE u.id IN ['U1', 'U100', 'U500', 'U999'] RETURN u.id, u.name ORDER BY u.id;

// 2.3 Range scan
MATCH (p:Product) WHERE p.price >= 50 AND p.price <= 100 RETURN count(p) AS priceRangeCount;

// 2.4 Prefix search (if supported)
MATCH (u:User) WHERE u.name STARTS WITH 'User1' RETURN count(u);

// =============================================================================
// 3. Aggregation Performance
// =============================================================================

// 3.1 Group by with count
MATCH (u:User) RETURN u.country, count(u) AS cnt ORDER BY cnt DESC;

// 3.2 Group by with multiple aggregations
MATCH (u:User) RETURN u.tier, count(u) AS cnt, avg(u.age) AS avgAge ORDER BY cnt DESC;

// 3.3 Product category statistics
MATCH (p:Product) RETURN p.category, count(p) AS cnt, avg(p.price) AS avgPrice, sum(p.stock) AS totalStock ORDER BY cnt DESC;

// 3.4 Large collect
MATCH (u:User {country: 'USA'}) WITH u ORDER BY u.id RETURN collect(u.name)[0..10] AS firstTenNames;

// =============================================================================
// 4. Join Performance
// =============================================================================

// 4.1 Simple two-hop pattern
MATCH (u:User {id: 'U1'})-[:FRIEND]->(friend)-[:FRIEND]->(fof)
WHERE fof <> u
RETURN DISTINCT fof.name
LIMIT 20;

// 4.2 Pattern with relationship properties
MATCH (u:User)-[p:PURCHASED]->(prod:Product)
WHERE p.quantity >= 3
RETURN u.name, prod.name, p.quantity
ORDER BY p.quantity DESC
LIMIT 20;

// 4.3 Multiple relationship types
MATCH (u:User)-[:PURCHASED]->(p:Product)<-[:REVIEWED]-(reviewer:User)
WHERE u <> reviewer
RETURN u.name AS buyer, p.name AS product, reviewer.name AS reviewer
LIMIT 20;

// =============================================================================
// 5. Path Finding Performance
// =============================================================================

// 5.1 Variable length paths (limited depth)
MATCH path = (u1:User {id: 'U1'})-[:FRIEND*1..3]->(u2:User)
RETURN length(path) AS depth, count(*) AS pathCount
ORDER BY depth;

// 5.2 Shortest path
MATCH path = shortestPath((u1:User {id: 'U1'})-[:FRIEND*]-(u2:User {id: 'U100'}))
RETURN length(path) AS distance, [n IN nodes(path) | n.name] AS names;

// 5.3 All shortest paths (limited)
MATCH path = allShortestPaths((u1:User {id: 'U1'})-[:FRIEND*]-(u2:User {id: 'U50'}))
RETURN length(path) AS distance, count(path) AS pathCount;

// =============================================================================
// 6. Subquery Performance
// =============================================================================

// 6.1 Correlated subquery
MATCH (u:User)
WHERE u.tier = 'platinum'
CALL {
  WITH u
  MATCH (u)-[:PURCHASED]->(p:Product)
  RETURN sum(p.price) AS totalSpent
}
RETURN u.name, totalSpent
ORDER BY totalSpent DESC
LIMIT 10;

// 6.2 Aggregation in subquery
MATCH (c:Category {level: 1})
CALL {
  WITH c
  MATCH (p:Product {category: c.name})
  RETURN count(p) AS productCount, avg(p.price) AS avgPrice
}
RETURN c.name, productCount, avgPrice
ORDER BY productCount DESC;

// =============================================================================
// 7. Sorting and Pagination
// =============================================================================

// 7.1 Large result set with ORDER BY
MATCH (u:User) RETURN u.name, u.age, u.country ORDER BY u.age DESC, u.name LIMIT 50;

// 7.2 SKIP and LIMIT (pagination)
MATCH (p:Product) RETURN p.name, p.price ORDER BY p.price DESC SKIP 100 LIMIT 20;

// 7.3 Complex ordering
MATCH (u:User)-[r:PURCHASED]->(p:Product)
RETURN u.name, p.name, r.quantity, r.price
ORDER BY r.quantity DESC, r.price DESC
LIMIT 50;

// =============================================================================
// 8. DISTINCT Performance
// =============================================================================

// 8.1 Simple distinct
MATCH (u:User) RETURN DISTINCT u.country ORDER BY u.country;

// 8.2 Distinct with pattern
MATCH (u:User)-[:PURCHASED]->(p:Product)
RETURN DISTINCT p.category
ORDER BY p.category;

// 8.3 Count distinct
MATCH (u:User)-[:PURCHASED]->(p:Product)
RETURN u.country, count(DISTINCT p.category) AS categoriesBought
ORDER BY categoriesBought DESC;

// =============================================================================
// 9. Complex Analytical Queries
// =============================================================================

// 9.1 Top buyers by total spending
MATCH (u:User)-[p:PURCHASED]->(prod:Product)
WITH u, sum(p.price * p.quantity) AS totalSpent
RETURN u.name, u.tier, totalSpent
ORDER BY totalSpent DESC
LIMIT 10;

// 9.3 Friend network analysis
MATCH (u:User {id: 'U1'})-[:FRIEND*1..2]->(friend)
WITH DISTINCT friend
RETURN friend.country, count(friend) AS cnt
ORDER BY cnt DESC;

// 9.4 Review quality analysis
MATCH (u:User)-[r:REVIEWED]->(p:Product)
WITH p, avg(r.rating) AS avgRating, count(r) AS reviewCount
WHERE reviewCount >= 5
RETURN p.name, avgRating, reviewCount
ORDER BY avgRating DESC, reviewCount DESC
LIMIT 20;

// =============================================================================
// 10. Write Performance
// =============================================================================

// 10.1 Bulk insert
UNWIND range(1001, 1100) AS i
CREATE (u:TestUser {id: 'TU' + toString(i), name: 'TestUser' + toString(i)})
RETURN count(*) AS inserted;

// 10.2 Bulk update
MATCH (u:User)
WHERE u.country = 'USA'
SET u.region = 'North America'
RETURN count(u) AS updated;

// 10.4 Cleanup test data
MATCH (u:TestUser) DETACH DELETE u RETURN count(*) AS deleted;
