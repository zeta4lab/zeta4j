// Subqueries Example - Data Setup
// CALL {} subquery testing

// Departments
CREATE (d1:Department {name: 'Engineering', budget: 500000})
CREATE (d2:Department {name: 'Sales', budget: 300000})
CREATE (d3:Department {name: 'Marketing', budget: 200000});

// Employees
CREATE (e1:Employee {name: 'Alice', salary: 80000, level: 'Senior'})
CREATE (e2:Employee {name: 'Bob', salary: 70000, level: 'Senior'})
CREATE (e3:Employee {name: 'Charlie', salary: 55000, level: 'Junior'})
CREATE (e4:Employee {name: 'Diana', salary: 90000, level: 'Lead'})
CREATE (e5:Employee {name: 'Eve', salary: 60000, level: 'Mid'})
CREATE (e6:Employee {name: 'Frank', salary: 50000, level: 'Junior'})
CREATE (e7:Employee {name: 'Grace', salary: 75000, level: 'Senior'})
CREATE (e8:Employee {name: 'Henry', salary: 65000, level: 'Mid'});

// Department memberships
MATCH (e:Employee {name: 'Alice'}), (d:Department {name: 'Engineering'}) CREATE (e)-[:WORKS_IN]->(d)
MATCH (e:Employee {name: 'Bob'}), (d:Department {name: 'Engineering'}) CREATE (e)-[:WORKS_IN]->(d)
MATCH (e:Employee {name: 'Charlie'}), (d:Department {name: 'Engineering'}) CREATE (e)-[:WORKS_IN]->(d)
MATCH (e:Employee {name: 'Diana'}), (d:Department {name: 'Engineering'}) CREATE (e)-[:WORKS_IN]->(d)
MATCH (e:Employee {name: 'Eve'}), (d:Department {name: 'Sales'}) CREATE (e)-[:WORKS_IN]->(d)
MATCH (e:Employee {name: 'Frank'}), (d:Department {name: 'Sales'}) CREATE (e)-[:WORKS_IN]->(d)
MATCH (e:Employee {name: 'Grace'}), (d:Department {name: 'Marketing'}) CREATE (e)-[:WORKS_IN]->(d)
MATCH (e:Employee {name: 'Henry'}), (d:Department {name: 'Marketing'}) CREATE (e)-[:WORKS_IN]->(d);

// Projects
CREATE (p1:Project {name: 'Alpha', status: 'active'})
CREATE (p2:Project {name: 'Beta', status: 'active'})
CREATE (p3:Project {name: 'Gamma', status: 'completed'});

// Project assignments
MATCH (e:Employee {name: 'Alice'}), (p:Project {name: 'Alpha'}) CREATE (e)-[:ASSIGNED_TO {role: 'Lead'}]->(p)
MATCH (e:Employee {name: 'Bob'}), (p:Project {name: 'Alpha'}) CREATE (e)-[:ASSIGNED_TO {role: 'Developer'}]->(p)
MATCH (e:Employee {name: 'Charlie'}), (p:Project {name: 'Alpha'}) CREATE (e)-[:ASSIGNED_TO {role: 'Developer'}]->(p)
MATCH (e:Employee {name: 'Diana'}), (p:Project {name: 'Beta'}) CREATE (e)-[:ASSIGNED_TO {role: 'Lead'}]->(p)
MATCH (e:Employee {name: 'Eve'}), (p:Project {name: 'Beta'}) CREATE (e)-[:ASSIGNED_TO {role: 'Sales'}]->(p)
MATCH (e:Employee {name: 'Grace'}), (p:Project {name: 'Gamma'}) CREATE (e)-[:ASSIGNED_TO {role: 'Marketing'}]->(p);

// Mentorship relationships
MATCH (a:Employee {name: 'Alice'}), (c:Employee {name: 'Charlie'}) CREATE (a)-[:MENTORS]->(c)
MATCH (d:Employee {name: 'Diana'}), (f:Employee {name: 'Frank'}) CREATE (d)-[:MENTORS]->(f)
MATCH (b:Employee {name: 'Bob'}), (h:Employee {name: 'Henry'}) CREATE (b)-[:MENTORS]->(h);
