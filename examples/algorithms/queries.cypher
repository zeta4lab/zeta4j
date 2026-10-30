// Graph Algorithm Examples
// Run after loading data.cypher
//
// Usage:
//   z4j shell -f examples/algorithms/queries.cypher

// =============================================================================
// Shortest Path Algorithms
// =============================================================================

// Find shortest path from Seoul to Busan (unweighted)
MATCH path = shortestPath((seoul:City {name: 'Seoul'})-[:ROAD*]-(busan:City {name: 'Busan'}))
RETURN [node IN nodes(path) | node.name] AS route, length(path) AS hops;

// Find all shortest paths from Seoul to Busan
MATCH path = allShortestPaths((seoul:City {name: 'Seoul'})-[:ROAD*]-(busan:City {name: 'Busan'}))
RETURN [node IN nodes(path) | node.name] AS route, length(path) AS hops;

// Find paths with distance limit
MATCH path = (seoul:City {name: 'Seoul'})-[:ROAD*1..4]-(city:City)
WHERE city.name <> 'Seoul'
RETURN DISTINCT city.name, length(path) AS hops
ORDER BY hops, city.name;

// Find reachable cities from Seoul within 3 hops
MATCH (seoul:City {name: 'Seoul'})-[:ROAD*1..3]-(city:City)
RETURN DISTINCT city.name AS reachableCity
ORDER BY reachableCity;

// =============================================================================
// Centrality Algorithms
// =============================================================================

// Degree Centrality (number of connections)
MATCH (c:City)
OPTIONAL MATCH (c)-[r:ROAD]-()
RETURN c.name, count(r) AS degree
ORDER BY degree DESC;

// In-Degree (for directed graphs - Pages)
MATCH (p:Page)
OPTIONAL MATCH ()-[:LINKS_TO]->(p)
RETURN p.title, count(*) AS inDegree
ORDER BY inDegree DESC;

// Out-Degree (for directed graphs - Pages)
MATCH (p:Page)
OPTIONAL MATCH (p)-[:LINKS_TO]->()
RETURN p.title, count(*) AS outDegree
ORDER BY outDegree DESC;

// Eigenvector-like centrality approximation (2-hop reach)
MATCH (c:City)-[:ROAD]-(neighbor)-[:ROAD]-(secondHop)
WHERE c <> secondHop
RETURN c.name, count(DISTINCT secondHop) AS reach
ORDER BY reach DESC;

// =============================================================================
// Community Detection (Pure Cypher alternatives)
// =============================================================================

// Find tightly connected groups via triangle membership
MATCH (a:City)-[:ROAD]-(b:City)-[:ROAD]-(c:City)-[:ROAD]-(a)
WHERE a.name < b.name AND b.name < c.name
RETURN a.name AS city1, b.name AS city2, c.name AS city3;

// Find bridge nodes (nodes connecting different groups)
MATCH (c:City)-[r:ROAD]-()
WITH c, count(r) AS degree
WHERE degree >= 2
MATCH (c)-[:ROAD]-(n1), (c)-[:ROAD]-(n2)
WHERE n1 <> n2 AND NOT EXISTS { MATCH (n1)-[:ROAD]-(n2) }
RETURN DISTINCT c.name AS bridgeNode, degree
ORDER BY degree DESC;

// =============================================================================
// Path Finding Algorithms
// =============================================================================

// BFS-like traversal from Seoul (using path length)
MATCH path = (start:City {name: 'Seoul'})-[:ROAD*1..3]-(city:City)
RETURN DISTINCT city.name, min(length(path)) AS depth
ORDER BY depth, city.name;

// Find all paths between two cities (limited)
MATCH path = (seoul:City {name: 'Seoul'})-[:ROAD*1..5]-(busan:City {name: 'Busan'})
RETURN [node IN nodes(path) | node.name] AS route, length(path) AS hops
ORDER BY hops
LIMIT 10;

// =============================================================================
// Tree Algorithms
// =============================================================================

// Tree traversal (Pre-order)
MATCH path = (root:TreeNode {id: 1})-[:CHILD*0..]->(node:TreeNode)
RETURN node.id, node.value, length(path) AS depth
ORDER BY node.id;

// Find leaves (nodes without children)
MATCH (node:TreeNode)
WHERE NOT EXISTS { MATCH (node)-[:CHILD]->(:TreeNode) }
RETURN node.id, node.value AS leaf;

// Find ancestors of a node
MATCH path = (ancestor:TreeNode)-[:CHILD*]->(node:TreeNode {id: 8})
RETURN [n IN nodes(path) | n.value] AS ancestorPath;

// Find subtree size
MATCH (root:TreeNode {id: 1})-[:CHILD*0..]->(node:TreeNode)
RETURN count(node) AS treeSize;

// Find tree height
MATCH path = (root:TreeNode {id: 1})-[:CHILD*]->(leaf:TreeNode)
WHERE NOT EXISTS { MATCH (leaf)-[:CHILD]->(:TreeNode) }
RETURN max(length(path)) AS treeHeight;

// =============================================================================
// Similarity Algorithms
// =============================================================================

// Jaccard Similarity (common neighbors / total neighbors)
MATCH (c1:City {name: 'Seoul'})-[:ROAD]-(neighbor)-[:ROAD]-(c2:City)
WHERE c1 <> c2
WITH c1, c2, count(DISTINCT neighbor) AS commonNeighbors
MATCH (c1)-[:ROAD]-(n1)
WITH c1, c2, commonNeighbors, count(DISTINCT n1) AS c1Neighbors
MATCH (c2)-[:ROAD]-(n2)
WITH c1, c2, commonNeighbors, c1Neighbors, count(DISTINCT n2) AS c2Neighbors
RETURN c2.name, commonNeighbors, toFloat(commonNeighbors) / (c1Neighbors + c2Neighbors - commonNeighbors) AS jaccardSimilarity
ORDER BY jaccardSimilarity DESC;

// Cosine Similarity on properties
MATCH (c1:City), (c2:City)
WHERE c1.name < c2.name AND c1.lat IS NOT NULL AND c2.lat IS NOT NULL
RETURN c1.name, c2.name, (c1.lat * c2.lat + c1.lng * c2.lng) / (sqrt(c1.lat * c1.lat + c1.lng * c1.lng) * sqrt(c2.lat * c2.lat + c2.lng * c2.lng)) AS cosineSimilarity
ORDER BY cosineSimilarity DESC
LIMIT 10;

// =============================================================================
// Link Prediction
// =============================================================================

// Common Neighbors (for link prediction)
MATCH (c1:City)-[:ROAD]-(common:City)-[:ROAD]-(c2:City)
WHERE c1.name < c2.name AND NOT EXISTS { MATCH (c1)-[:ROAD]-(c2) }
RETURN c1.name, c2.name, count(common) AS commonNeighbors
ORDER BY commonNeighbors DESC;

// Adamic-Adar Index
MATCH (c1:City)-[:ROAD]-(common:City)-[:ROAD]-(c2:City)
WHERE c1.name < c2.name AND NOT EXISTS { MATCH (c1)-[:ROAD]-(c2) }
WITH c1, c2, common
MATCH (common)-[:ROAD]-(neighbor)
WITH c1, c2, common, count(neighbor) AS degree
WHERE degree > 1
RETURN c1.name, c2.name, sum(1.0 / log(toFloat(degree))) AS adamicAdar
ORDER BY adamicAdar DESC;

// =============================================================================
// Network Statistics
// =============================================================================

// Graph density
MATCH (c:City)
WITH count(c) AS nodeCount
MATCH ()-[r:ROAD]->()
WITH nodeCount, count(r) AS edgeCount
RETURN nodeCount, edgeCount, toFloat(edgeCount) / (nodeCount * (nodeCount - 1)) AS density;

// Average degree
MATCH (c:City)
OPTIONAL MATCH (c)-[r:ROAD]-()
WITH c, count(r) AS degree
RETURN avg(degree) AS avgDegree, max(degree) AS maxDegree, min(degree) AS minDegree;

// Diameter (longest shortest path)
MATCH (c1:City), (c2:City)
WHERE c1 <> c2
MATCH path = shortestPath((c1)-[:ROAD*]-(c2))
RETURN max(length(path)) AS diameter;

// Average path length
MATCH (c1:City), (c2:City)
WHERE c1 <> c2
MATCH path = shortestPath((c1)-[:ROAD*]-(c2))
RETURN avg(length(path)) AS avgPathLength;
