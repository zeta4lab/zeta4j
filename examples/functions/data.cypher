// Functions Example - Data Setup
// Testing all Cypher function categories

// Text data for string functions
CREATE (t1:Text {content: 'Hello World', code: 'ABC123'})
CREATE (t2:Text {content: '  trimmed  ', code: 'xyz789'})
CREATE (t3:Text {content: 'CamelCaseText', code: 'TEST_CODE'});

// Numeric data for math functions
CREATE (n1:Number {value: 3.14159, integer: 42})
CREATE (n2:Number {value: -7.5, integer: -10})
CREATE (n3:Number {value: 0.0, integer: 0})
CREATE (n4:Number {value: 100.999, integer: 1000});

// List data for list functions
CREATE (l1:ListData {items: [1, 2, 3, 4, 5], names: ['Alice', 'Bob', 'Charlie']})
CREATE (l2:ListData {items: [10, 20, 30], names: ['X', 'Y']})
CREATE (l3:ListData {items: [], names: ['Single']});

// Date data for temporal functions
CREATE (d1:Event {name: 'Meeting', year: 2024, month: 6, day: 15})
CREATE (d2:Event {name: 'Conference', year: 2024, month: 12, day: 25})
CREATE (d3:Event {name: 'Deadline', year: 2025, month: 1, day: 1});

// Mixed data for type functions
CREATE (m1:Mixed {text: 'hello', number: 42, flag: true, nothing: null})
CREATE (m2:Mixed {text: '123', number: 3.14, flag: false});

// Graph structure for path functions (single statement to preserve variable references)
CREATE (a:Node {name: 'A'})-[:CONNECTS {weight: 1}]->(b:Node {name: 'B'}), (b)-[:CONNECTS {weight: 2}]->(c:Node {name: 'C'}), (c)-[:CONNECTS {weight: 3}]->(d:Node {name: 'D'}), (a)-[:CONNECTS {weight: 10}]->(d);

// Products for aggregation functions
CREATE (p1:Product {name: 'Widget', price: 9.99, quantity: 100, category: 'A'})
CREATE (p2:Product {name: 'Gadget', price: 29.99, quantity: 50, category: 'A'})
CREATE (p3:Product {name: 'Gizmo', price: 19.99, quantity: 75, category: 'B'})
CREATE (p4:Product {name: 'Thingamajig', price: 49.99, quantity: 25, category: 'B'})
CREATE (p5:Product {name: 'Doohickey', price: 14.99, quantity: 200, category: 'C'});
