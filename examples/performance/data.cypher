// Performance Example - Data Setup
// Testing with large data volumes

// =============================================================================
// 1. Generate Users (1000 nodes)
// =============================================================================

UNWIND range(1, 1000) AS i CREATE (u:User {id: 'U' + toString(i), name: 'User' + toString(i), email: 'user' + toString(i) + '@example.com', age: 18 + (i % 50), country: CASE i % 5 WHEN 0 THEN 'USA' WHEN 1 THEN 'UK' WHEN 2 THEN 'Germany' WHEN 3 THEN 'Japan' ELSE 'Korea' END, tier: CASE WHEN i <= 100 THEN 'platinum' WHEN i <= 300 THEN 'gold' WHEN i <= 600 THEN 'silver' ELSE 'bronze' END, createdAt: timestamp() - (i * 86400000)});

// =============================================================================
// 2. Generate Products (500 nodes)
// =============================================================================

UNWIND range(1, 500) AS i CREATE (p:Product {id: 'P' + toString(i), name: 'Product' + toString(i), category: CASE i % 10 WHEN 0 THEN 'Electronics' WHEN 1 THEN 'Clothing' WHEN 2 THEN 'Books' WHEN 3 THEN 'Home' WHEN 4 THEN 'Sports' WHEN 5 THEN 'Toys' WHEN 6 THEN 'Food' WHEN 7 THEN 'Beauty' WHEN 8 THEN 'Auto' ELSE 'Other' END, price: 9.99 + (i % 100) * 10, stock: i * 10, rating: 1.0 + (i % 50) / 10.0});

// =============================================================================
// 3. Generate Friendships (random connections, ~5000 edges)
// =============================================================================

// Create friendship groups
UNWIND range(1, 100) AS cluster UNWIND range(1, 10) AS member MATCH (u1:User {id: 'U' + toString((cluster - 1) * 10 + member)}) MATCH (u2:User {id: 'U' + toString((cluster - 1) * 10 + ((member % 10) + 1))}) WHERE u1 <> u2 CREATE (u1)-[:FRIEND {since: 2020 + (cluster % 5)}]->(u2);

// Cross-group friendships
UNWIND range(1, 500) AS i MATCH (u1:User {id: 'U' + toString(i)}) MATCH (u2:User {id: 'U' + toString(((i * 7) % 1000) + 1)}) WHERE u1 <> u2 MERGE (u1)-[:FRIEND]->(u2);

// =============================================================================
// 4. Generate Purchases (~3000 edges)
// =============================================================================

UNWIND range(1, 1000) AS userId UNWIND range(1, 3) AS purchaseNum MATCH (u:User {id: 'U' + toString(userId)}) MATCH (p:Product {id: 'P' + toString(((userId + purchaseNum * 13) % 500) + 1)}) CREATE (u)-[:PURCHASED {quantity: 1 + (purchaseNum % 5), price: p.price, date: timestamp() - ((userId + purchaseNum) * 3600000)}]->(p);

// =============================================================================
// 5. Generate Reviews (~2000 edges)
// =============================================================================

UNWIND range(1, 500) AS userId UNWIND range(1, 4) AS reviewNum MATCH (u:User {id: 'U' + toString(userId)}) MATCH (p:Product {id: 'P' + toString(((userId * 3 + reviewNum) % 500) + 1)}) CREATE (u)-[:REVIEWED {rating: 1 + (reviewNum % 5), text: 'Review text ' + toString(reviewNum), helpful: reviewNum * 10}]->(p);

// =============================================================================
// 6. Generate Categories Hierarchy
// =============================================================================

CREATE (root:Category {name: 'All Products', level: 0});

UNWIND ['Electronics', 'Clothing', 'Books', 'Home', 'Sports'] AS catName CREATE (c:Category {name: catName, level: 1});

MATCH (root:Category {name: 'All Products'}), (c:Category {level: 1}) CREATE (root)-[:HAS_SUBCATEGORY]->(c);

// Subcategories for Electronics
MATCH (c:Category {name: 'Electronics'}) UNWIND ['Phones', 'Laptops', 'Tablets', 'Cameras'] AS sub CREATE (s:Category {name: sub, level: 2}) CREATE (c)-[:HAS_SUBCATEGORY]->(s);

// Subcategories for Clothing
MATCH (c:Category {name: 'Clothing'}) UNWIND ['Mens', 'Womens', 'Kids', 'Accessories'] AS sub CREATE (s:Category {name: sub, level: 2}) CREATE (c)-[:HAS_SUBCATEGORY]->(s);
