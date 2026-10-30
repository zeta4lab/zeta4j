// Social Network Query Examples
// Run after loading data.cypher
//
// Usage:
//   z4j shell -f examples/social/queries.cypher

// =============================================================================
// Basic Social Queries
// =============================================================================

// Find all users
MATCH (u:User) RETURN u.name, u.age, u.city;

// Find users in Seoul
MATCH (u:User {city: 'Seoul'}) RETURN u.name, u.occupation;

// Find users by occupation
MATCH (u:User) WHERE u.occupation = 'Developer' RETURN u.name, u.city;

// =============================================================================
// Friend Relationships
// =============================================================================

// Find Alice's friends
MATCH (alice:User {name: 'Alice'})-[:FRIEND]->(friend:User)
RETURN friend.name, friend.city;

// Find friends of friends (2nd degree)
MATCH (alice:User {name: 'Alice'})-[:FRIEND]->(:User)-[:FRIEND]->(fof:User)
WHERE fof.name <> 'Alice'
RETURN DISTINCT fof.name AS friendOfFriend;

// Find mutual friends between Alice and Charlie
MATCH (alice:User {name: 'Alice'})-[:FRIEND]->(mutual:User)<-[:FRIEND]-(charlie:User {name: 'Charlie'})
RETURN mutual.name AS mutualFriend;

// Find users with most friends
MATCH (u:User)-[:FRIEND]-(friend:User)
RETURN u.name, count(DISTINCT friend) AS friendCount
ORDER BY friendCount DESC
LIMIT 5;

// =============================================================================
// Follower Analysis
// =============================================================================

// Find who follows Nick
MATCH (follower:User)-[:FOLLOWS]->(nick:User {name: 'Nick'})
RETURN follower.name;

// Find users with most followers
MATCH (user:User)<-[:FOLLOWS]-(follower:User)
RETURN user.name, count(follower) AS followerCount
ORDER BY followerCount DESC;

// Find influencers (high follower to following ratio)
MATCH (u:User)
OPTIONAL MATCH (u)<-[:FOLLOWS]-(follower:User)
OPTIONAL MATCH (u)-[:FOLLOWS]->(following:User)
WITH u, count(DISTINCT follower) AS followers, count(DISTINCT following) AS following
WHERE followers > 0
RETURN u.name, followers, following,
       toFloat(followers) / CASE WHEN following = 0 THEN 1 ELSE following END AS ratio
ORDER BY ratio DESC;

// =============================================================================
// Company Analysis
// =============================================================================

// Find all employees of TechCorp
MATCH (u:User)-[w:WORKS_AT]->(c:Company {name: 'TechCorp'})
RETURN u.name, w.role, w.since;

// Find colleagues (same company)
MATCH (alice:User {name: 'Alice'})-[:WORKS_AT]->(c:Company)<-[:WORKS_AT]-(colleague:User)
WHERE colleague.name <> 'Alice'
RETURN colleague.name, c.name AS company;

// Find companies with most employees
MATCH (u:User)-[:WORKS_AT]->(c:Company)
RETURN c.name, count(u) AS employeeCount
ORDER BY employeeCount DESC;

// Find cross-company connections
MATCH (u1:User)-[:WORKS_AT]->(c1:Company),
      (u1)-[:FRIEND]-(u2:User)-[:WORKS_AT]->(c2:Company)
WHERE c1 <> c2
RETURN c1.name AS company1, c2.name AS company2, count(*) AS connections
ORDER BY connections DESC;

// =============================================================================
// Interest-based Queries
// =============================================================================

// Find users interested in Coding
MATCH (u:User)-[i:INTERESTED_IN]->(interest:Interest {name: 'Coding'})
RETURN u.name, i.level;

// Find common interests between users
MATCH (alice:User {name: 'Alice'})-[:INTERESTED_IN]->(i:Interest)<-[:INTERESTED_IN]-(other:User)
WHERE other.name <> 'Alice'
RETURN other.name, collect(i.name) AS commonInterests;

// Recommend friends based on common interests
MATCH (alice:User {name: 'Alice'})-[:INTERESTED_IN]->(i:Interest)<-[:INTERESTED_IN]-(potential:User)
WHERE NOT EXISTS { MATCH (alice)-[:FRIEND]-(potential) }
  AND alice <> potential
RETURN potential.name, count(i) AS commonInterests, collect(i.name) AS interests
ORDER BY commonInterests DESC;

// Find interest popularity
MATCH (u:User)-[:INTERESTED_IN]->(i:Interest)
RETURN i.name, i.category, count(u) AS userCount
ORDER BY userCount DESC;

// =============================================================================
// Path Queries
// =============================================================================

// Find shortest path between two users
MATCH path = shortestPath(
  (alice:User {name: 'Alice'})-[:FRIEND*]-(nick:User {name: 'Nick'})
)
RETURN [node IN nodes(path) | node.name] AS path, length(path) AS distance;

// Find all paths up to 3 hops
MATCH path = (alice:User {name: 'Alice'})-[:FRIEND*..3]-(other:User)
WHERE alice <> other
RETURN other.name, length(path) AS distance
ORDER BY distance;

// =============================================================================
// Community Detection Patterns
// =============================================================================

// Find tightly connected groups (cliques)
MATCH (u1:User)-[:FRIEND]-(u2:User)-[:FRIEND]-(u3:User)-[:FRIEND]-(u1)
WHERE u1.name < u2.name AND u2.name < u3.name
RETURN u1.name, u2.name, u3.name AS triangle;

// Find bridge nodes (connecting different communities)
MATCH (u:User)-[:FRIEND]-(friend:User)
WITH u, count(DISTINCT friend) AS degree
WHERE degree >= 3
MATCH (u)-[:FRIEND]-(f1:User)-[:FRIEND]-(f2:User)-[:FRIEND]-(u)
WITH u, degree, count(*) AS triangles
RETURN u.name, degree, triangles,
       toFloat(triangles) / (degree * (degree - 1) / 2) AS clusteringCoeff
ORDER BY clusteringCoeff;

// =============================================================================
// Complex Recommendations
// =============================================================================

// Friend recommendation (friends of friends who share interests)
MATCH (me:User {name: 'Alice'})-[:FRIEND]-(:User)-[:FRIEND]-(fof:User)
WHERE fof <> me AND NOT EXISTS { MATCH (me)-[:FRIEND]-(fof) }
OPTIONAL MATCH (me)-[:INTERESTED_IN]->(i:Interest)<-[:INTERESTED_IN]-(fof)
WITH fof, count(i) AS commonInterests
RETURN fof.name, commonInterests
ORDER BY commonInterests DESC
LIMIT 5;

// Job recommendation based on similar users
MATCH (me:User {name: 'Kate'})-[:INTERESTED_IN]->(i:Interest)<-[:INTERESTED_IN]-(similar:User)
WHERE similar <> me
MATCH (similar)-[w:WORKS_AT]->(c:Company)
RETURN c.name, w.role, count(DISTINCT similar) AS similarUsers
ORDER BY similarUsers DESC;

// Find mentors (older users with same interests and higher position)
MATCH (me:User {name: 'Kate'})-[:INTERESTED_IN]->(i:Interest)<-[:INTERESTED_IN]-(mentor:User)
WHERE mentor.age > me.age + 5
MATCH (mentor)-[w:WORKS_AT]->(c:Company)
WHERE w.role CONTAINS 'Senior' OR w.role CONTAINS 'Lead' OR w.role CONTAINS 'Manager'
RETURN mentor.name, mentor.age, w.role, collect(i.name) AS sharedInterests;

// =============================================================================
// City-based Analysis
// =============================================================================

// Find users per city
MATCH (u:User)
RETURN u.city, count(u) AS userCount, collect(u.name) AS users
ORDER BY userCount DESC;

// Find cross-city friendships
MATCH (u1:User)-[:FRIEND]-(u2:User)
WHERE u1.city <> u2.city
RETURN u1.city, u2.city, count(*) AS connections
ORDER BY connections DESC;

// Find city hubs (users with friends in multiple cities)
MATCH (u:User)-[:FRIEND]-(friend:User)
WITH u, collect(DISTINCT friend.city) AS friendCities
WHERE size(friendCities) > 1
RETURN u.name, u.city, friendCities, size(friendCities) AS cityReach
ORDER BY cityReach DESC;
