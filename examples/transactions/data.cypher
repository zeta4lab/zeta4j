// Transactions Example - Data Setup
// Testing transaction isolation and ACID properties

// Bank accounts for transfer tests
CREATE (a1:Account {id: 'ACC001', owner: 'Alice', balance: 1000})
CREATE (a2:Account {id: 'ACC002', owner: 'Bob', balance: 500})
CREATE (a3:Account {id: 'ACC003', owner: 'Charlie', balance: 2000});

// Inventory for concurrent update tests
CREATE (i1:Inventory {sku: 'PROD001', name: 'Widget', stock: 100})
CREATE (i2:Inventory {sku: 'PROD002', name: 'Gadget', stock: 50})
CREATE (i3:Inventory {sku: 'PROD003', name: 'Gizmo', stock: 0});

// Counter for increment tests
CREATE (c:Counter {name: 'visits', value: 0})
CREATE (c2:Counter {name: 'orders', value: 0});

// Lock test data
CREATE (r:Resource {name: 'shared_resource', locked: false, owner: null});

// Versioned data for optimistic locking tests
CREATE (d:Document {id: 'DOC001', content: 'Initial content', version: 1})
CREATE (d2:Document {id: 'DOC002', content: 'Another document', version: 1});

// Queue for FIFO tests
CREATE (q:Queue {name: 'tasks', items: []});

// Sequence generator
CREATE (s:Sequence {name: 'order_id', current: 1000});
