// Subqueries Example - Query Tests
// CALL {} subquery patterns

// =============================================================================
// 1. Basic CALL subquery - aggregate in subquery
// =============================================================================

// 1.1 Count employees per department using subquery
MATCH (d:Department)
CALL {
  WITH d
  MATCH (e:Employee)-[:WORKS_IN]->(d)
  RETURN count(e) AS empCount
}
RETURN d.name AS department, empCount
ORDER BY d.name;

// 1.2 Average salary per department using subquery
MATCH (d:Department)
CALL {
  WITH d
  MATCH (e:Employee)-[:WORKS_IN]->(d)
  RETURN avg(e.salary) AS avgSalary
}
RETURN d.name AS department, avgSalary
ORDER BY d.name;

// =============================================================================
// 2. CALL subquery with filtering
// =============================================================================

// 2.1 Departments with high-salary employees
MATCH (d:Department)
CALL {
  WITH d
  MATCH (e:Employee)-[:WORKS_IN]->(d)
  WHERE e.salary > 70000
  RETURN collect(e.name) AS highEarners
}
RETURN d.name AS department, highEarners
ORDER BY d.name;

// 2.2 Find top earner per department
MATCH (d:Department)
CALL {
  WITH d
  MATCH (e:Employee)-[:WORKS_IN]->(d)
  RETURN e.name AS topEarner, e.salary AS maxSalary
  ORDER BY e.salary DESC
  LIMIT 1
}
RETURN d.name AS department, topEarner, maxSalary
ORDER BY d.name;

// =============================================================================
// 3. CALL subquery with relationships
// =============================================================================

// 3.1 Find employees and their project count
MATCH (e:Employee)
CALL {
  WITH e
  MATCH (e)-[:ASSIGNED_TO]->(p:Project)
  RETURN count(p) AS projectCount
}
RETURN e.name, projectCount
ORDER BY e.name;

// 3.2 Find mentors with mentee count
MATCH (mentor:Employee)
CALL {
  WITH mentor
  MATCH (mentor)-[:MENTORS]->(mentee:Employee)
  RETURN count(mentee) AS menteeCount
}
WITH mentor, menteeCount
WHERE menteeCount > 0
RETURN mentor.name, menteeCount
ORDER BY mentor.name;

// =============================================================================
// 4. Nested aggregation with CALL
// =============================================================================

// 4.1 Department summary with multiple stats
MATCH (d:Department)
CALL {
  WITH d
  MATCH (e:Employee)-[:WORKS_IN]->(d)
  RETURN count(e) AS empCount, sum(e.salary) AS totalSalary, avg(e.salary) AS avgSalary
}
RETURN d.name AS department, empCount, totalSalary, avgSalary
ORDER BY d.name;

// 4.2 Project statistics
MATCH (p:Project)
CALL {
  WITH p
  MATCH (e:Employee)-[:ASSIGNED_TO]->(p)
  RETURN count(e) AS teamSize, collect(e.name) AS team
}
RETURN p.name AS project, p.status, teamSize, team
ORDER BY p.name;

// =============================================================================
// 5. CALL with UNION (if supported)
// =============================================================================

// 5.1 Employees from active projects
MATCH (e:Employee)
CALL {
  WITH e
  MATCH (e)-[:ASSIGNED_TO]->(p:Project {status: 'active'})
  RETURN p.name AS projectName
}
RETURN DISTINCT e.name, projectName
ORDER BY e.name, projectName;

// =============================================================================
// 6. Complex patterns with CALL
// =============================================================================

// 6.1 Find employees who are both mentors and project leads
MATCH (e:Employee)
CALL {
  WITH e
  MATCH (e)-[:MENTORS]->(:Employee)
  RETURN true AS isMentor
}
CALL {
  WITH e
  MATCH (e)-[a:ASSIGNED_TO]->(:Project)
  WHERE a.role = 'Lead'
  RETURN true AS isLead
}
RETURN e.name
ORDER BY e.name;

// 6.2 Department with project participation
MATCH (d:Department)
CALL {
  WITH d
  MATCH (e:Employee)-[:WORKS_IN]->(d)
  MATCH (e)-[:ASSIGNED_TO]->(p:Project)
  RETURN count(DISTINCT p) AS projectCount
}
RETURN d.name AS department, projectCount
ORDER BY d.name;
