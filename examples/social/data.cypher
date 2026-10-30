// Social Network Dataset
// Sample social graph for testing relationships and community detection
//
// Usage:
//   z4j shell -f examples/social/data.cypher

// =============================================================================
// Create Users
// =============================================================================

CREATE (alice:User {name: 'Alice', age: 28, city: 'Seoul', occupation: 'Engineer'});
CREATE (bob:User {name: 'Bob', age: 32, city: 'Seoul', occupation: 'Designer'});
CREATE (charlie:User {name: 'Charlie', age: 25, city: 'Busan', occupation: 'Developer'});
CREATE (diana:User {name: 'Diana', age: 30, city: 'Seoul', occupation: 'Manager'});
CREATE (eve:User {name: 'Eve', age: 27, city: 'Incheon', occupation: 'Developer'});
CREATE (frank:User {name: 'Frank', age: 35, city: 'Busan', occupation: 'Architect'});
CREATE (grace:User {name: 'Grace', age: 29, city: 'Seoul', occupation: 'Designer'});
CREATE (henry:User {name: 'Henry', age: 31, city: 'Daegu', occupation: 'Engineer'});
CREATE (ivy:User {name: 'Ivy', age: 26, city: 'Seoul', occupation: 'Analyst'});
CREATE (jack:User {name: 'Jack', age: 33, city: 'Busan', occupation: 'Manager'});
CREATE (kate:User {name: 'Kate', age: 24, city: 'Seoul', occupation: 'Intern'});
CREATE (leo:User {name: 'Leo', age: 28, city: 'Incheon', occupation: 'Developer'});
CREATE (mia:User {name: 'Mia', age: 30, city: 'Seoul', occupation: 'Designer'});
CREATE (nick:User {name: 'Nick', age: 34, city: 'Daegu', occupation: 'CTO'});
CREATE (olivia:User {name: 'Olivia', age: 27, city: 'Seoul', occupation: 'HR'});

// =============================================================================
// Create Companies
// =============================================================================

CREATE (techcorp:Company {name: 'TechCorp', industry: 'Technology', employees: 500});
CREATE (designhub:Company {name: 'DesignHub', industry: 'Design', employees: 50});
CREATE (startupabc:Company {name: 'StartupABC', industry: 'Technology', employees: 20});
CREATE (bigfinance:Company {name: 'BigFinance', industry: 'Finance', employees: 1000});

// =============================================================================
// Create Interest Groups
// =============================================================================

CREATE (coding:Interest {name: 'Coding', category: 'Technology'});
CREATE (design:Interest {name: 'Design', category: 'Creative'});
CREATE (hiking:Interest {name: 'Hiking', category: 'Sports'});
CREATE (gaming:Interest {name: 'Gaming', category: 'Entertainment'});
CREATE (cooking:Interest {name: 'Cooking', category: 'Lifestyle'});
CREATE (music:Interest {name: 'Music', category: 'Entertainment'});
CREATE (photography:Interest {name: 'Photography', category: 'Creative'});

// =============================================================================
// Create FRIEND Relationships (bidirectional in nature)
// =============================================================================

MATCH (a:User {name: 'Alice'}), (b:User {name: 'Bob'})
CREATE (a)-[:FRIEND {since: 2018}]->(b);

MATCH (a:User {name: 'Alice'}), (c:User {name: 'Charlie'})
CREATE (a)-[:FRIEND {since: 2019}]->(c);

MATCH (a:User {name: 'Alice'}), (d:User {name: 'Diana'})
CREATE (a)-[:FRIEND {since: 2020}]->(d);

MATCH (b:User {name: 'Bob'}), (g:User {name: 'Grace'})
CREATE (b)-[:FRIEND {since: 2017}]->(g);

MATCH (b:User {name: 'Bob'}), (m:User {name: 'Mia'})
CREATE (b)-[:FRIEND {since: 2019}]->(m);

MATCH (c:User {name: 'Charlie'}), (e:User {name: 'Eve'})
CREATE (c)-[:FRIEND {since: 2018}]->(e);

MATCH (c:User {name: 'Charlie'}), (f:User {name: 'Frank'})
CREATE (c)-[:FRIEND {since: 2020}]->(f);

MATCH (d:User {name: 'Diana'}), (h:User {name: 'Henry'})
CREATE (d)-[:FRIEND {since: 2019}]->(h);

MATCH (d:User {name: 'Diana'}), (o:User {name: 'Olivia'})
CREATE (d)-[:FRIEND {since: 2021}]->(o);

MATCH (e:User {name: 'Eve'}), (l:User {name: 'Leo'})
CREATE (e)-[:FRIEND {since: 2017}]->(l);

MATCH (f:User {name: 'Frank'}), (j:User {name: 'Jack'})
CREATE (f)-[:FRIEND {since: 2018}]->(j);

MATCH (g:User {name: 'Grace'}), (i:User {name: 'Ivy'})
CREATE (g)-[:FRIEND {since: 2019}]->(i);

MATCH (h:User {name: 'Henry'}), (n:User {name: 'Nick'})
CREATE (h)-[:FRIEND {since: 2016}]->(n);

MATCH (i:User {name: 'Ivy'}), (k:User {name: 'Kate'})
CREATE (i)-[:FRIEND {since: 2020}]->(k);

MATCH (j:User {name: 'Jack'}), (n:User {name: 'Nick'})
CREATE (j)-[:FRIEND {since: 2015}]->(n);

MATCH (k:User {name: 'Kate'}), (o:User {name: 'Olivia'})
CREATE (k)-[:FRIEND {since: 2021}]->(o);

MATCH (l:User {name: 'Leo'}), (m:User {name: 'Mia'})
CREATE (l)-[:FRIEND {since: 2019}]->(m);

// =============================================================================
// Create FOLLOWS Relationships (directed)
// =============================================================================

MATCH (a:User {name: 'Alice'}), (n:User {name: 'Nick'})
CREATE (a)-[:FOLLOWS]->(n);

MATCH (b:User {name: 'Bob'}), (n:User {name: 'Nick'})
CREATE (b)-[:FOLLOWS]->(n);

MATCH (c:User {name: 'Charlie'}), (a:User {name: 'Alice'})
CREATE (c)-[:FOLLOWS]->(a);

MATCH (k:User {name: 'Kate'}), (a:User {name: 'Alice'})
CREATE (k)-[:FOLLOWS]->(a);

MATCH (k:User {name: 'Kate'}), (b:User {name: 'Bob'})
CREATE (k)-[:FOLLOWS]->(b);

MATCH (i:User {name: 'Ivy'}), (d:User {name: 'Diana'})
CREATE (i)-[:FOLLOWS]->(d);

// =============================================================================
// Create WORKS_AT Relationships
// =============================================================================

MATCH (a:User {name: 'Alice'}), (c:Company {name: 'TechCorp'})
CREATE (a)-[:WORKS_AT {role: 'Senior Engineer', since: 2020}]->(c);

MATCH (b:User {name: 'Bob'}), (c:Company {name: 'DesignHub'})
CREATE (b)-[:WORKS_AT {role: 'Lead Designer', since: 2018}]->(c);

MATCH (c:User {name: 'Charlie'}), (s:Company {name: 'StartupABC'})
CREATE (c)-[:WORKS_AT {role: 'Backend Developer', since: 2021}]->(s);

MATCH (d:User {name: 'Diana'}), (t:Company {name: 'TechCorp'})
CREATE (d)-[:WORKS_AT {role: 'Engineering Manager', since: 2019}]->(t);

MATCH (e:User {name: 'Eve'}), (s:Company {name: 'StartupABC'})
CREATE (e)-[:WORKS_AT {role: 'Frontend Developer', since: 2020}]->(s);

MATCH (n:User {name: 'Nick'}), (s:Company {name: 'StartupABC'})
CREATE (n)-[:WORKS_AT {role: 'CTO', since: 2019}]->(s);

MATCH (i:User {name: 'Ivy'}), (b:Company {name: 'BigFinance'})
CREATE (i)-[:WORKS_AT {role: 'Data Analyst', since: 2021}]->(b);

MATCH (g:User {name: 'Grace'}), (d:Company {name: 'DesignHub'})
CREATE (g)-[:WORKS_AT {role: 'UI Designer', since: 2019}]->(d);

// =============================================================================
// Create INTERESTED_IN Relationships
// =============================================================================

MATCH (a:User {name: 'Alice'}), (i:Interest {name: 'Coding'})
CREATE (a)-[:INTERESTED_IN {level: 'Expert'}]->(i);

MATCH (a:User {name: 'Alice'}), (i:Interest {name: 'Hiking'})
CREATE (a)-[:INTERESTED_IN {level: 'Intermediate'}]->(i);

MATCH (b:User {name: 'Bob'}), (i:Interest {name: 'Design'})
CREATE (b)-[:INTERESTED_IN {level: 'Expert'}]->(i);

MATCH (b:User {name: 'Bob'}), (i:Interest {name: 'Photography'})
CREATE (b)-[:INTERESTED_IN {level: 'Advanced'}]->(i);

MATCH (c:User {name: 'Charlie'}), (i:Interest {name: 'Coding'})
CREATE (c)-[:INTERESTED_IN {level: 'Advanced'}]->(i);

MATCH (c:User {name: 'Charlie'}), (i:Interest {name: 'Gaming'})
CREATE (c)-[:INTERESTED_IN {level: 'Expert'}]->(i);

MATCH (d:User {name: 'Diana'}), (i:Interest {name: 'Hiking'})
CREATE (d)-[:INTERESTED_IN {level: 'Advanced'}]->(i);

MATCH (e:User {name: 'Eve'}), (i:Interest {name: 'Coding'})
CREATE (e)-[:INTERESTED_IN {level: 'Intermediate'}]->(i);

MATCH (e:User {name: 'Eve'}), (i:Interest {name: 'Music'})
CREATE (e)-[:INTERESTED_IN {level: 'Beginner'}]->(i);

MATCH (g:User {name: 'Grace'}), (i:Interest {name: 'Design'})
CREATE (g)-[:INTERESTED_IN {level: 'Expert'}]->(i);

MATCH (g:User {name: 'Grace'}), (i:Interest {name: 'Photography'})
CREATE (g)-[:INTERESTED_IN {level: 'Advanced'}]->(i);

MATCH (h:User {name: 'Henry'}), (i:Interest {name: 'Hiking'})
CREATE (h)-[:INTERESTED_IN {level: 'Expert'}]->(i);

MATCH (i:User {name: 'Ivy'}), (x:Interest {name: 'Cooking'})
CREATE (i)-[:INTERESTED_IN {level: 'Advanced'}]->(x);

MATCH (k:User {name: 'Kate'}), (x:Interest {name: 'Music'})
CREATE (k)-[:INTERESTED_IN {level: 'Intermediate'}]->(x);

MATCH (l:User {name: 'Leo'}), (x:Interest {name: 'Coding'})
CREATE (l)-[:INTERESTED_IN {level: 'Expert'}]->(x);

MATCH (l:User {name: 'Leo'}), (x:Interest {name: 'Gaming'})
CREATE (l)-[:INTERESTED_IN {level: 'Advanced'}]->(x);

MATCH (m:User {name: 'Mia'}), (x:Interest {name: 'Design'})
CREATE (m)-[:INTERESTED_IN {level: 'Expert'}]->(x);

MATCH (n:User {name: 'Nick'}), (x:Interest {name: 'Coding'})
CREATE (n)-[:INTERESTED_IN {level: 'Expert'}]->(x);
