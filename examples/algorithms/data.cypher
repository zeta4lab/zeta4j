// Graph Algorithm Test Dataset
// A weighted graph for testing various graph algorithms
//
// Usage:
//   z4j shell -f examples/algorithms/data.cypher

// =============================================================================
// Create a weighted road network graph
// =============================================================================

// Cities (Nodes)
CREATE (seoul:City {name: 'Seoul', population: 9700000, lat: 37.5665, lng: 126.9780});
CREATE (busan:City {name: 'Busan', population: 3400000, lat: 35.1796, lng: 129.0756});
CREATE (incheon:City {name: 'Incheon', population: 2900000, lat: 37.4563, lng: 126.7052});
CREATE (daegu:City {name: 'Daegu', population: 2400000, lat: 35.8714, lng: 128.6014});
CREATE (daejeon:City {name: 'Daejeon', population: 1500000, lat: 36.3504, lng: 127.3845});
CREATE (gwangju:City {name: 'Gwangju', population: 1500000, lat: 35.1595, lng: 126.8526});
CREATE (ulsan:City {name: 'Ulsan', population: 1100000, lat: 35.5384, lng: 129.3114});
CREATE (suwon:City {name: 'Suwon', population: 1200000, lat: 37.2636, lng: 127.0286});
CREATE (jeonju:City {name: 'Jeonju', population: 650000, lat: 35.8242, lng: 127.1480});
CREATE (cheongju:City {name: 'Cheongju', population: 850000, lat: 36.6424, lng: 127.4890});
CREATE (chuncheon:City {name: 'Chuncheon', population: 280000, lat: 37.8813, lng: 127.7298});
CREATE (gangneung:City {name: 'Gangneung', population: 210000, lat: 37.7519, lng: 128.8761});

// Roads (Edges with distance in km)
MATCH (seoul:City {name: 'Seoul'}), (busan:City {name: 'Busan'})
CREATE (seoul)-[:ROAD {distance: 325, type: 'highway'}]->(busan);

MATCH (seoul:City {name: 'Seoul'}), (incheon:City {name: 'Incheon'})
CREATE (seoul)-[:ROAD {distance: 28, type: 'highway'}]->(incheon);

MATCH (seoul:City {name: 'Seoul'}), (suwon:City {name: 'Suwon'})
CREATE (seoul)-[:ROAD {distance: 30, type: 'highway'}]->(suwon);

MATCH (seoul:City {name: 'Seoul'}), (chuncheon:City {name: 'Chuncheon'})
CREATE (seoul)-[:ROAD {distance: 75, type: 'highway'}]->(chuncheon);

MATCH (seoul:City {name: 'Seoul'}), (daejeon:City {name: 'Daejeon'})
CREATE (seoul)-[:ROAD {distance: 140, type: 'highway'}]->(daejeon);

MATCH (suwon:City {name: 'Suwon'}), (daejeon:City {name: 'Daejeon'})
CREATE (suwon)-[:ROAD {distance: 110, type: 'highway'}]->(daejeon);

MATCH (daejeon:City {name: 'Daejeon'}), (daegu:City {name: 'Daegu'})
CREATE (daejeon)-[:ROAD {distance: 120, type: 'highway'}]->(daegu);

MATCH (daejeon:City {name: 'Daejeon'}), (jeonju:City {name: 'Jeonju'})
CREATE (daejeon)-[:ROAD {distance: 55, type: 'highway'}]->(jeonju);

MATCH (daejeon:City {name: 'Daejeon'}), (cheongju:City {name: 'Cheongju'})
CREATE (daejeon)-[:ROAD {distance: 35, type: 'local'}]->(cheongju);

MATCH (cheongju:City {name: 'Cheongju'}), (seoul:City {name: 'Seoul'})
CREATE (cheongju)-[:ROAD {distance: 130, type: 'highway'}]->(seoul);

MATCH (daegu:City {name: 'Daegu'}), (busan:City {name: 'Busan'})
CREATE (daegu)-[:ROAD {distance: 88, type: 'highway'}]->(busan);

MATCH (daegu:City {name: 'Daegu'}), (ulsan:City {name: 'Ulsan'})
CREATE (daegu)-[:ROAD {distance: 70, type: 'highway'}]->(ulsan);

MATCH (ulsan:City {name: 'Ulsan'}), (busan:City {name: 'Busan'})
CREATE (ulsan)-[:ROAD {distance: 55, type: 'highway'}]->(busan);

MATCH (jeonju:City {name: 'Jeonju'}), (gwangju:City {name: 'Gwangju'})
CREATE (jeonju)-[:ROAD {distance: 75, type: 'highway'}]->(gwangju);

MATCH (gwangju:City {name: 'Gwangju'}), (busan:City {name: 'Busan'})
CREATE (gwangju)-[:ROAD {distance: 200, type: 'highway'}]->(busan);

MATCH (chuncheon:City {name: 'Chuncheon'}), (gangneung:City {name: 'Gangneung'})
CREATE (chuncheon)-[:ROAD {distance: 95, type: 'local'}]->(gangneung);

MATCH (gangneung:City {name: 'Gangneung'}), (seoul:City {name: 'Seoul'})
CREATE (gangneung)-[:ROAD {distance: 165, type: 'highway'}]->(seoul);

// =============================================================================
// Create a simple tree structure for tree algorithms
// =============================================================================

CREATE (root:TreeNode {id: 1, value: 'Root'});
CREATE (a:TreeNode {id: 2, value: 'A'});
CREATE (b:TreeNode {id: 3, value: 'B'});
CREATE (c:TreeNode {id: 4, value: 'C'});
CREATE (d:TreeNode {id: 5, value: 'D'});
CREATE (e:TreeNode {id: 6, value: 'E'});
CREATE (f:TreeNode {id: 7, value: 'F'});
CREATE (g:TreeNode {id: 8, value: 'G'});

MATCH (root:TreeNode {id: 1}), (a:TreeNode {id: 2})
CREATE (root)-[:CHILD]->(a);

MATCH (root:TreeNode {id: 1}), (b:TreeNode {id: 3})
CREATE (root)-[:CHILD]->(b);

MATCH (a:TreeNode {id: 2}), (c:TreeNode {id: 4})
CREATE (a)-[:CHILD]->(c);

MATCH (a:TreeNode {id: 2}), (d:TreeNode {id: 5})
CREATE (a)-[:CHILD]->(d);

MATCH (b:TreeNode {id: 3}), (e:TreeNode {id: 6})
CREATE (b)-[:CHILD]->(e);

MATCH (b:TreeNode {id: 3}), (f:TreeNode {id: 7})
CREATE (b)-[:CHILD]->(f);

MATCH (d:TreeNode {id: 5}), (g:TreeNode {id: 8})
CREATE (d)-[:CHILD]->(g);

// =============================================================================
// Create a web graph for PageRank
// =============================================================================

CREATE (home:Page {url: '/home', title: 'Home'});
CREATE (about:Page {url: '/about', title: 'About'});
CREATE (products:Page {url: '/products', title: 'Products'});
CREATE (p1:Page {url: '/products/1', title: 'Product 1'});
CREATE (p2:Page {url: '/products/2', title: 'Product 2'});
CREATE (p3:Page {url: '/products/3', title: 'Product 3'});
CREATE (blog:Page {url: '/blog', title: 'Blog'});
CREATE (post1:Page {url: '/blog/post1', title: 'Post 1'});
CREATE (post2:Page {url: '/blog/post2', title: 'Post 2'});
CREATE (contact:Page {url: '/contact', title: 'Contact'});

// Links
MATCH (home:Page {url: '/home'}), (about:Page {url: '/about'})
CREATE (home)-[:LINKS_TO]->(about);

MATCH (home:Page {url: '/home'}), (products:Page {url: '/products'})
CREATE (home)-[:LINKS_TO]->(products);

MATCH (home:Page {url: '/home'}), (blog:Page {url: '/blog'})
CREATE (home)-[:LINKS_TO]->(blog);

MATCH (home:Page {url: '/home'}), (contact:Page {url: '/contact'})
CREATE (home)-[:LINKS_TO]->(contact);

MATCH (about:Page {url: '/about'}), (home:Page {url: '/home'})
CREATE (about)-[:LINKS_TO]->(home);

MATCH (products:Page {url: '/products'}), (home:Page {url: '/home'})
CREATE (products)-[:LINKS_TO]->(home);

MATCH (products:Page {url: '/products'}), (p1:Page {url: '/products/1'})
CREATE (products)-[:LINKS_TO]->(p1);

MATCH (products:Page {url: '/products'}), (p2:Page {url: '/products/2'})
CREATE (products)-[:LINKS_TO]->(p2);

MATCH (products:Page {url: '/products'}), (p3:Page {url: '/products/3'})
CREATE (products)-[:LINKS_TO]->(p3);

MATCH (p1:Page {url: '/products/1'}), (products:Page {url: '/products'})
CREATE (p1)-[:LINKS_TO]->(products);

MATCH (p2:Page {url: '/products/2'}), (products:Page {url: '/products'})
CREATE (p2)-[:LINKS_TO]->(products);

MATCH (p3:Page {url: '/products/3'}), (products:Page {url: '/products'})
CREATE (p3)-[:LINKS_TO]->(products);

MATCH (p1:Page {url: '/products/1'}), (p2:Page {url: '/products/2'})
CREATE (p1)-[:LINKS_TO]->(p2);

MATCH (p2:Page {url: '/products/2'}), (p3:Page {url: '/products/3'})
CREATE (p2)-[:LINKS_TO]->(p3);

MATCH (blog:Page {url: '/blog'}), (home:Page {url: '/home'})
CREATE (blog)-[:LINKS_TO]->(home);

MATCH (blog:Page {url: '/blog'}), (post1:Page {url: '/blog/post1'})
CREATE (blog)-[:LINKS_TO]->(post1);

MATCH (blog:Page {url: '/blog'}), (post2:Page {url: '/blog/post2'})
CREATE (blog)-[:LINKS_TO]->(post2);

MATCH (post1:Page {url: '/blog/post1'}), (blog:Page {url: '/blog'})
CREATE (post1)-[:LINKS_TO]->(blog);

MATCH (post1:Page {url: '/blog/post1'}), (post2:Page {url: '/blog/post2'})
CREATE (post1)-[:LINKS_TO]->(post2);

MATCH (post2:Page {url: '/blog/post2'}), (blog:Page {url: '/blog'})
CREATE (post2)-[:LINKS_TO]->(blog);

MATCH (contact:Page {url: '/contact'}), (home:Page {url: '/home'})
CREATE (contact)-[:LINKS_TO]->(home);
