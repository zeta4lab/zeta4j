# Zeta4j

Zeta4j is a graph database written in Rust. It stores a property graph in a
single embedded [redb](https://github.com/cberner/redb) file per database,
answers [openCypher](https://opencypher.org/) queries over the Bolt protocol and
an HTTP API, and keeps fulltext (Tantivy) and vector (HNSW) indexes in the same
transaction as the graph data.

- openCypher queries, transactions and schema commands
- Bolt protocol, HTTP transactional API, gRPC administration
- Fulltext indexes with BM25 scoring and language analyzers
- Vector indexes (HNSW, cosine / euclidean / dot product)
- Multiple logical databases on one server
- Users with password authentication and Ed25519-signed session tokens
- Optional TLS on every listener

Default ports: HTTP `9044`, Bolt `9045`, gRPC `9046`.

## Build

Requires Rust 1.99 or newer and `protoc` (Debian/Ubuntu: `protobuf-compiler`).

```bash
cargo build --release -p zeta4j -p zeta4j-cli
```

This produces two binaries in `target/release`:

| Binary    | Purpose                                                 |
|-----------|---------------------------------------------------------|
| `zeta4jd` | the database server                                     |
| `z4j`     | server control, Cypher shell, authentication, admin     |

Put both on your `PATH`, or keep them side by side: `z4j` finds `zeta4jd` next
to itself.

## Run

The server signs session tokens with an Ed25519 key, and a fresh database
needs a one-time bootstrap credential for the first user.

```bash
export ZETA4J_HOME="$HOME/.zeta4j"

# 1. Token signing key (PKCS#8 DER, owner-only)
mkdir -p "$ZETA4J_HOME"
openssl genpkey -algorithm ed25519 -outform DER -out "$ZETA4J_HOME/jwt-signer.pk8"
chmod 600 "$ZETA4J_HOME/jwt-signer.pk8"
export ZETA4J_JWT_SIGNING_KEY_FILE="$ZETA4J_HOME/jwt-signer.pk8"

# 2. Initialize storage and generate the bootstrap credential
z4j server init --generate-bootstrap-credential --bootstrap-output "$ZETA4J_HOME/bootstrap.toml"
export ZETA4J_BOOTSTRAP_CREDENTIAL_FILE="$ZETA4J_HOME/bootstrap.toml"

# 3. Start
z4j server start
z4j server status
```

`bootstrap.toml` holds a generated username and password. Replace the password
once to activate the account:

```bash
curl -s http://127.0.0.1:9044/auth/bootstrap/complete \
  -H 'Content-Type: application/json' \
  -d '{"username":"<username>","current_password":"<password>","new_password":"<new password, 12+ chars>"}'
```

After that the bootstrap file is no longer used and can be deleted. Stop the
server with `z4j server stop`; `z4j server console` runs it in the foreground.

The bootstrap credential expires 15 minutes after
`z4j server init --generate-bootstrap-credential` creates it, so start the
server and complete the bootstrap within that window. An expired credential can
no longer start a fresh database or complete the bootstrap. To get a new one,
initialize again. `--clean` deletes all data and generated configuration in
`$ZETA4J_HOME` (the token signing key is kept), and `--bootstrap-output` must
name a file that does not exist yet:

```bash
z4j server stop
rm "$ZETA4J_HOME/bootstrap.toml"
z4j server init --clean -y --generate-bootstrap-credential --bootstrap-output "$ZETA4J_HOME/bootstrap.toml"
z4j server start
```

For a throwaway local instance without authentication:

```bash
z4j server init
z4j server start --profile local-test --no-auth
```

Authentication can only be disabled with the `local-test` profile on loopback
addresses.

## Query

### Cypher shell (Bolt)

```bash
z4j shell --username <username> --password <password>
z4j shell --username <username> --password <password> "MATCH (n) RETURN count(n)"
```

### HTTP

Log in to get an access token, then post statements to
`/db/<database>/tx/commit`. The default database is `zeta4j`. Reuse the token
for all requests until it expires (`expires_in` seconds in the login response)
instead of logging in for every request, and log out when you are done:

```bash
TOKEN=$(curl -s http://127.0.0.1:9044/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"<username>","password":"<password>"}' | jq -r .access_token)

curl -s http://127.0.0.1:9044/db/zeta4j/tx/commit \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"statements":[{"statement":"MATCH (n:Person) RETURN n.name"}]}'

curl -s -X POST http://127.0.0.1:9044/auth/logout -H "Authorization: Bearer $TOKEN"
```

### Graph

```cypher
CREATE (ada:Person {name: 'Ada', bio: 'analytical engine pioneer'})
CREATE (grace:Person {name: 'Grace', bio: 'compiler pioneer'})
CREATE (ada)-[:KNOWS {since: 1843}]->(grace);

MATCH (a:Person)-[:KNOWS]->(b) RETURN a.name, b.name;
```

### Fulltext

```cypher
CREATE FULLTEXT INDEX person_text FOR (n:Person) ON EACH [n.name, n.bio] ANALYZER english;

CALL db.index.fulltext.queryNodes('person_text', 'compiler')
YIELD node, score
RETURN node.name, score;
```

### Vector

```cypher
MATCH (p:Person {name: 'Ada'}) SET p.embedding = vector([1.0, 0.0, 0.0]);
MATCH (p:Person {name: 'Grace'}) SET p.embedding = vector([0.0, 1.0, 0.0]);

CREATE VECTOR INDEX person_embedding FOR (n:Person) ON (n.embedding)
DIMENSIONS 3 DISTANCE cosine;

CALL db.index.vector.queryNodes('person_embedding', 1, [0.9, 0.1, 0.0])
YIELD node, score
RETURN node.name, score;
```

More query samples are in [`examples/`](examples/).

## Configuration

`z4j server init` writes `core.toml` and `server.toml` to
`$ZETA4J_HOME/config`. The environment variables below, and the matching
`z4j server start` flags such as `--http-port`, cover the common settings;
everything else is set in `server.toml` and `core.toml`. Where a setting can be
given in more than one place, command-line flags win over environment
variables, which win over the files.

| Variable                           | Meaning                                  |
|------------------------------------|------------------------------------------|
| `ZETA4J_HOME`                      | home directory (default `~/.zeta4j`)     |
| `ZETA4J_BIND_ADDRESS`              | listen address (default `127.0.0.1`)     |
| `ZETA4J_HTTP_PORT`, `ZETA4J_BOLT_PORT`, `ZETA4J_GRPC_PORT` | listener ports |
| `ZETA4J_JWT_SIGNING_KEY_FILE`      | Ed25519 PKCS#8 DER token signing key     |
| `ZETA4J_BOOTSTRAP_CREDENTIAL_FILE` | bootstrap envelope for a fresh database  |
| `ZETA4J_BOOTSTRAP_USERNAME`, `ZETA4J_BOOTSTRAP_PASSWORD` | bootstrap credential given directly |
| `ZETA4J_TRANSPORT_MODE`            | `plaintext` (default) or `tls`           |
| `ZETA4J_TRANSPORT_SERVER_CERT_REF` | `file:<path>` to the PEM certificate chain |
| `ZETA4J_TRANSPORT_SERVER_KEY_REF`  | `file:<path>` to the PEM private key (mode 0600) |

### TLS

With `ZETA4J_TRANSPORT_MODE=tls` the HTTP, Bolt and gRPC listeners all require
TLS 1.3 using the given certificate and key. Clients choose TLS by URL scheme
(`https://`, `bolt+s://`) and trust the roots in
`ZETA4J_CLIENT_SERVER_ROOTS_REF`:

```bash
export ZETA4J_CLIENT_SERVER_ROOTS_REF=file:/path/to/ca.crt
z4j shell --address bolt+s://db.example.com:9045 --username <username> --password <password>
```

## Docker

```bash
docker build -f docker/zeta4j.Dockerfile -t zeta4j .
ZETA4J_BOOTSTRAP_PASSWORD='<12+ chars>' docker compose up -d
```

The container creates its token signing key in the data volume on first start
and bootstraps the user `admin` with the password from
`ZETA4J_BOOTSTRAP_PASSWORD` (`docker-compose.yml` sets
`ZETA4J_BOOTSTRAP_USERNAME=admin`). Complete the bootstrap as shown above
within 15 minutes of that first start. The compose file requires
`ZETA4J_BOOTSTRAP_PASSWORD` to be set for every `docker compose` command, but
the value is only used while the database has no users.

If the bootstrap window expires, recreate the data volume, which deletes all
data:

```bash
ZETA4J_BOOTSTRAP_PASSWORD='<12+ chars>' docker compose down -v
ZETA4J_BOOTSTRAP_PASSWORD='<12+ chars>' docker compose up -d
```

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/e2e-smoke.sh   # after a release build
```

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
