// Functions Example - Query Tests
// Comprehensive function testing

// =============================================================================
// 1. String Functions
// =============================================================================

// 1.1 Case conversion
MATCH (t:Text) RETURN t.content, toUpper(t.content) AS upper, toLower(t.content) AS lower ORDER BY t.content;

// 1.2 Trim functions
MATCH (t:Text) WHERE t.content CONTAINS 'trim' RETURN trim(t.content) AS trimmed, ltrim(t.content) AS ltrimmed, rtrim(t.content) AS rtrimmed;

// 1.3 Substring and Left/Right
MATCH (t:Text) RETURN t.content, substring(t.content, 0, 5) AS first5, left(t.content, 3) AS left3, right(t.content, 3) AS right3 ORDER BY t.content;

// 1.4 Replace and Reverse
MATCH (t:Text {content: 'Hello World'}) RETURN replace(t.content, 'World', 'Cypher') AS replaced, reverse(t.content) AS reversed;

// 1.5 Split
MATCH (t:Text {content: 'Hello World'}) RETURN split(t.content, ' ') AS words;

// 1.6 toString
MATCH (n:Number) RETURN n.integer, toString(n.integer) AS asString ORDER BY n.integer;

// =============================================================================
// 2. Math Functions
// =============================================================================

// 2.1 Basic math
MATCH (n:Number) RETURN n.value, abs(n.value) AS absVal, sign(n.value) AS signVal ORDER BY n.value;

// 2.2 Rounding
MATCH (n:Number) WHERE n.value <> 0 RETURN n.value, ceil(n.value) AS ceiling, floor(n.value) AS flooring, round(n.value) AS rounded ORDER BY n.value;

// 2.3 Power and roots
MATCH (n:Number) WHERE n.value > 0 RETURN n.value, sqrt(n.value) AS sqrtVal, n.value ^ 2 AS squared ORDER BY n.value;

// 2.4 Logarithms
MATCH (n:Number) WHERE n.value > 0 RETURN n.value, log(n.value) AS naturalLog, log10(n.value) AS log10Val ORDER BY n.value;

// 2.5 Trigonometric
RETURN sin(0) AS sin0, cos(0) AS cos0, tan(0) AS tan0, pi() AS piVal, e() AS eVal;

// 2.6 Angle conversion
RETURN degrees(pi()) AS piInDegrees, radians(180) AS halfCircleRadians;

// 2.7 Random
RETURN rand() AS random1, rand() AS random2;

// =============================================================================
// 3. List Functions
// =============================================================================

// 3.1 Head, Tail, Last
MATCH (l:ListData) RETURN l.items, head(l.items) AS first, last(l.items) AS lastItem ORDER BY size(l.items);

// 3.2 Size
MATCH (l:ListData) RETURN l.items, size(l.items) AS itemCount, l.names, size(l.names) AS nameCount ORDER BY size(l.items);

// 3.3 Tail (rest of list)
MATCH (l:ListData) WHERE size(l.items) > 0 RETURN l.items, tail(l.items) AS rest ORDER BY size(l.items);

// 3.4 Range
RETURN range(1, 5) AS oneToFive, range(0, 10, 2) AS evenNumbers, range(5, 1, -1) AS countdown;

// 3.5 Keys (from map)
MATCH (m:Mixed) RETURN keys(m) AS propKeys ORDER BY m.text;

// =============================================================================
// 4. Type Conversion Functions
// =============================================================================

// 4.1 toInteger
MATCH (m:Mixed) RETURN m.text, toInteger(m.text) AS asInt ORDER BY m.text;

// 4.2 toFloat
MATCH (m:Mixed) RETURN m.number, toFloat(toString(m.number)) AS backToFloat ORDER BY m.number;

// 4.3 toBoolean
RETURN toBoolean('true') AS t, toBoolean('false') AS f, toBoolean('TRUE') AS T;

// 4.4 Coalesce
MATCH (m:Mixed) RETURN coalesce(m.nothing, m.text, 'default') AS result ORDER BY m.text;

// 4.5 randomUUID
RETURN randomUUID() AS uuid1, randomUUID() AS uuid2;

// 4.6 timestamp
RETURN timestamp() AS now;

// =============================================================================
// 5. Graph Element Functions
// =============================================================================

// 5.1 id()
MATCH (n:Node) RETURN n.name, id(n) AS nodeId ORDER BY n.name;

// 5.2 labels()
MATCH (t:Text) RETURN t.content, labels(t) AS nodeLabels ORDER BY t.content;

// 5.3 type()
MATCH ()-[r:CONNECTS]->() RETURN type(r) AS relType LIMIT 1;

// 5.4 properties()
MATCH (n:Node {name: 'A'}) RETURN properties(n) AS props;

// =============================================================================
// 6. Path Functions
// =============================================================================

// 6.1 nodes()
MATCH p = (a:Node {name: 'A'})-[*]->(d:Node {name: 'D'})
RETURN [n IN nodes(p) | n.name] AS pathNodes
ORDER BY length(p);

// 6.2 relationships()
MATCH p = (a:Node {name: 'A'})-[*]->(d:Node {name: 'D'})
RETURN length(p) AS pathLength, [r IN relationships(p) | r.weight] AS weights
ORDER BY length(p);

// 6.3 length()
MATCH p = (a:Node {name: 'A'})-[*]->(d:Node {name: 'D'})
RETURN length(p) AS hops
ORDER BY length(p);

// =============================================================================
// 7. Aggregation Functions
// =============================================================================

// 7.1 count
MATCH (p:Product) RETURN count(p) AS total, count(p.category) AS withCategory;

// 7.2 sum, avg, min, max
MATCH (p:Product) RETURN sum(p.price) AS totalPrice, avg(p.price) AS avgPrice, min(p.price) AS minPrice, max(p.price) AS maxPrice;

// 7.3 collect
MATCH (p:Product) WITH p ORDER BY p.name RETURN collect(p.name) AS productNames;

// 7.4 Group by with aggregation
MATCH (p:Product) RETURN p.category, count(p) AS cnt, sum(p.price) AS total ORDER BY p.category;

// =============================================================================
// 8. Temporal Functions
// =============================================================================

// 8.1 date()
RETURN date() AS today, date('2024-06-15') AS specificDate;

// 8.2 datetime()
RETURN datetime() AS now;

// 8.3 date components
MATCH (e:Event) RETURN e.name, date({year: e.year, month: e.month, day: e.day}) AS eventDate ORDER BY e.name;

// =============================================================================
// 9. Spatial Functions
// =============================================================================

// =============================================================================
// 10. Combined/Nested Functions
// =============================================================================

// 10.1 Nested string functions
MATCH (t:Text) RETURN trim(toLower(t.content)) AS cleaned ORDER BY t.content;

// 10.2 Math with aggregation
MATCH (p:Product) RETURN round(avg(p.price) * 100) / 100 AS avgRounded;

// 10.3 List comprehension with functions
MATCH (l:ListData) WHERE size(l.items) > 0
RETURN [x IN l.items WHERE x > 2 | x * 2] AS doubled ORDER BY size(l.items) DESC;

// 10.4 Complex aggregation
MATCH (p:Product)
WITH p.category AS cat, collect(p.name) AS names, avg(p.price) AS avgPrice
RETURN cat, size(names) AS count, names, round(avgPrice * 100) / 100 AS avg
ORDER BY cat;
