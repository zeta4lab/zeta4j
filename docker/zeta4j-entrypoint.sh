#!/bin/sh
# Creates the token signing key in the data volume on first start.
set -eu
key="${ZETA4J_JWT_SIGNING_KEY_FILE:-$ZETA4J_HOME/jwt-signer.pk8}"
if [ ! -f "$key" ]; then
    (umask 077 && openssl genpkey -algorithm ed25519 -outform DER -out "$key")
fi
export ZETA4J_JWT_SIGNING_KEY_FILE="$key"
exec "$@"
