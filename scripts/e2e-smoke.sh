#!/usr/bin/env bash
# End-to-end smoke test against locally built release binaries.
#
#   cargo build --release -p zeta4j -p zeta4j-cli
#   scripts/e2e-smoke.sh
#
# Requires curl, jq and openssl. Uses a throwaway home directory and ports.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="${ZETA4J_BIN_DIR:-$root/target/release}"
z4j="$bin/z4j"
for tool in curl jq openssl "$z4j" "$bin/zeta4jd"; do
    command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 1; }
done

work="$(mktemp -d)"
export ZETA4J_HOME="$work/home"
export ZETA4J_SERVER_PATH="$bin/zeta4jd"
base=$((20000 + RANDOM % 20000))
http_port=$base bolt_port=$((base + 1)) grpc_port=$((base + 2))
ports=(--http-port "$http_port" --bolt-port "$bolt_port" --grpc-port "$grpc_port")

cleanup() {
    "$z4j" server stop >/dev/null 2>&1 || true
    rm -rf "$work"
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
step() { echo "==> $*"; }

scheme=http
curl_tls=()
api() { # api METHOD PATH [JSON] [TOKEN]
    local auth=()
    [[ -n "${4:-}" ]] && auth=(-H "Authorization: Bearer $4")
    curl -sS "${curl_tls[@]}" -X "$1" "$scheme://127.0.0.1:$http_port$2" \
        -H 'Content-Type: application/json' "${auth[@]}" ${3:+-d "$3"}
}
cypher() { # cypher TOKEN STATEMENT -> rows as JSON
    local body response
    body="$(jq -n --arg s "$2" '{statements: [{statement: $s}]}')"
    response="$(api POST /db/zeta4j/tx/commit "$body" "$1")"
    [[ "$(jq '.errors | length' <<<"$response")" == 0 ]] || fail "query '$2': $response"
    jq -c '[.results[0].data[].row]' <<<"$response"
}

step "init"
openssl genpkey -algorithm ed25519 -outform DER -out "$work/signer.pk8" 2>/dev/null
chmod 600 "$work/signer.pk8"
export ZETA4J_JWT_SIGNING_KEY_FILE="$work/signer.pk8"
"$z4j" server init --clean -y --generate-bootstrap-credential \
    --bootstrap-output "$work/bootstrap.toml" >/dev/null
export ZETA4J_BOOTSTRAP_CREDENTIAL_FILE="$work/bootstrap.toml"
username="$(sed -n 's/^username = "\(.*\)"$/\1/p' "$work/bootstrap.toml")"
initial_password="$(sed -n 's/^password = "\(.*\)"$/\1/p' "$work/bootstrap.toml")"
password="smoke-$(openssl rand -hex 12)"

step "start (authentication on)"
"$z4j" server start "${ports[@]}" >/dev/null

step "unauthenticated query is rejected"
status="$(curl -s -o /dev/null -w '%{http_code}' -X POST \
    "http://127.0.0.1:$http_port/db/zeta4j/tx/commit" -H 'Content-Type: application/json' \
    -d '{"statements":[{"statement":"RETURN 1"}]}')"
[[ "$status" == 401 ]] || fail "expected 401, got $status"

login() {
    api POST /auth/login "$(jq -n --arg u "$username" --arg p "$password" \
        '{username: $u, password: $p}')" | jq -r .access_token
}
admin_status() { # admin_status GRPC_ADDRESS -> fails unless the gRPC admin answers
    printf '%s' "$password" | "$z4j" auth --endpoint "$scheme://127.0.0.1:$http_port" login \
        --username "$username" --password-stdin >/dev/null
    "$z4j" admin --address "$1" status >/dev/null
}

step "complete bootstrap"
status="$(curl -s -o /dev/null -w '%{http_code}' -X POST \
    "http://127.0.0.1:$http_port/auth/bootstrap/complete" -H 'Content-Type: application/json' \
    -d "$(jq -n --arg u "$username" --arg c "$initial_password" --arg n "$password" \
        '{username: $u, current_password: $c, new_password: $n}')")"
[[ "$status" == 204 ]] || fail "bootstrap completion returned $status"
token="$(login)"
[[ -n "$token" && "$token" != null ]] || fail "login returned no token"

step "server and admin status over plaintext"
"$z4j" server status >/dev/null || fail "server status (plaintext)"
admin_status "localhost:$grpc_port" || fail "admin status over plaintext gRPC to a hostname"

step "graph writes and reads"
cypher "$token" "CREATE (:Person {name: 'Ada', bio: 'analytical engine pioneer', embedding: vector([1.0, 0.0, 0.0])})-[:KNOWS]->(:Person {name: 'Grace', bio: 'compiler pioneer', embedding: vector([0.0, 1.0, 0.0])})" >/dev/null
graph="$(cypher "$token" "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name, b.name")"
[[ "$graph" == '[["Ada","Grace"]]' ]] || fail "graph read: $graph"

step "fulltext index"
cypher "$token" "CREATE FULLTEXT INDEX person_text FOR (n:Person) ON EACH [n.name, n.bio] ANALYZER english" >/dev/null
fulltext_query="CALL db.index.fulltext.queryNodes('person_text', 'compiler') YIELD node RETURN node.name"
fulltext="$(cypher "$token" "$fulltext_query")"
[[ "$fulltext" == '[["Grace"]]' ]] || fail "fulltext: $fulltext"

step "vector index"
cypher "$token" "CREATE VECTOR INDEX person_embedding FOR (n:Person) ON (n.embedding) DIMENSIONS 3 DISTANCE cosine" >/dev/null
vector_query="CALL db.index.vector.queryNodes('person_embedding', 1, [0.9, 0.1, 0.0]) YIELD node RETURN node.name"
vector="$(cypher "$token" "$vector_query")"
[[ "$vector" == '[["Ada"]]' ]] || fail "vector: $vector"

step "restart keeps data and indexes"
"$z4j" server restart "${ports[@]}" >/dev/null
token="$(login)"
[[ "$(cypher "$token" "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name, b.name")" == "$graph" ]] \
    || fail "graph changed across restart"
[[ "$(cypher "$token" "$fulltext_query")" == "$fulltext" ]] || fail "fulltext changed across restart"
[[ "$(cypher "$token" "$vector_query")" == "$vector" ]] || fail "vector changed across restart"
"$z4j" server stop >/dev/null

step "TLS with a private CA"
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 1 \
    -subj /CN=smoke-ca -keyout "$work/ca.key" -out "$work/ca.crt" 2>/dev/null
openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
    -subj /CN=localhost -keyout "$work/server.key" -out "$work/server.csr" 2>/dev/null
printf 'basicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n' \
    >"$work/server.ext"
openssl x509 -req -in "$work/server.csr" -CA "$work/ca.crt" -CAkey "$work/ca.key" \
    -CAcreateserial -days 1 -extfile "$work/server.ext" -out "$work/server.crt" 2>/dev/null
chmod 600 "$work/server.key"
export ZETA4J_TRANSPORT_MODE=tls
export ZETA4J_TRANSPORT_SERVER_CERT_REF="file:$work/server.crt"
export ZETA4J_TRANSPORT_SERVER_KEY_REF="file:$work/server.key"
export ZETA4J_CLIENT_SERVER_ROOTS_REF="file:$work/ca.crt"
"$z4j" server start "${ports[@]}" >/dev/null
scheme=https
curl_tls=(--cacert "$work/ca.crt")
token="$(login)"
[[ "$(cypher "$token" "$fulltext_query")" == "$fulltext" ]] || fail "HTTPS query"
bolt="$("$z4j" shell --address "bolt+s://localhost:$bolt_port" --username "$username" \
    --password "$password" --format json "MATCH (p:Person) RETURN count(p) AS people")"
grep -q '"people": *2' <<<"$bolt" || fail "Bolt TLS query: $bolt"
"$z4j" server status >/dev/null || fail "server status (TLS)"
admin_status "https://localhost:$grpc_port" || fail "admin status over gRPC TLS"
"$z4j" server stop >/dev/null

echo "PASS"
