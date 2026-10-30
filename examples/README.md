# Zeta4j Examples

Sample Cypher datasets and queries for learning and trying out Zeta4j. Each
directory has a `data.cypher` file that loads a small graph and a
`queries.cypher` file that queries it (`crud/` has one file per operation
instead).

| Directory       | Contents                                                                                   |
|-----------------|--------------------------------------------------------------------------------------------|
| `movies/`       | Movies, actors and directors (`Movie`, `Person`, `ACTED_IN`, `DIRECTED`); pattern matching, paths, aggregation |
| `social/`       | Users, companies and interests (`FRIEND`, `FOLLOWS`, `WORKS_AT`, `INTERESTED_IN`); recommendations and community queries |
| `algorithms/`   | A weighted road network, a tree and a web link graph; shortest paths, reachability, degree and triangle queries |
| `crud/`         | `create.cypher`, `read.cypher`, `update.cypher`, `delete.cypher`: basic write and read patterns (run `create.cypher` first) |
| `subqueries/`   | Departments, employees and projects; `CALL { ... }` subqueries, correlated and nested aggregation |
| `functions/`    | String, math, list, type conversion, graph element, path, aggregation, temporal and spatial functions |
| `transactions/` | Accounts, inventory, counters and documents; atomic updates, optimistic and pessimistic locking patterns |
| `concurrency/`  | Ticket booking, stock, rate limiting, job queue and lock patterns built from conditional updates |
| `performance/`  | 1,000 users, 500 products and several thousand relationships; scans, aggregation, paths, sorting and pagination |

## Start a server

For a throwaway local instance without authentication:

```bash
z4j server init
z4j server start --profile local-test --no-auth
```

See the [main README](../README.md) for an authenticated setup. Against an
authenticated server, add `--username <username> --password <password>` to the
`z4j shell` commands below.

## Run the examples

`z4j shell -f <file>` runs a script file statement by statement. Statements must
end with `;`, and execution stops at the first statement that fails. Every file
in this directory is written this way and runs end to end:

```bash
z4j shell -f examples/movies/data.cypher
z4j shell -f examples/movies/queries.cypher
```

A single statement can also be passed as an argument, or typed into the
interactive shell:

```bash
z4j shell "MATCH (a:Account) RETURN a.id, a.owner, a.balance ORDER BY a.id"

z4j shell
zeta4j> MATCH (e:Employee) RETURN e.name LIMIT 5;
```

Statements can also be piped to the shell, each ending with `;`:

```bash
echo "MATCH (m:Movie) RETURN m.title LIMIT 5;" | z4j shell
```

## Reset the data

The datasets reuse labels such as `User` and `Product`, so clear the database
before loading a different one:

```bash
z4j shell "MATCH (n) DETACH DELETE n"
```

## HTTP

The same statements can be sent to the HTTP transactional endpoint. On a
`local-test` server started with `--no-auth`, no token is needed:

```bash
curl -s http://127.0.0.1:9044/db/zeta4j/tx/commit \
  -H 'Content-Type: application/json' \
  -d '{"statements":[{"statement":"MATCH (m:Movie) RETURN m.title LIMIT 5"}]}'
```
