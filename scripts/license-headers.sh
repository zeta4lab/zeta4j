#!/usr/bin/env bash
# Copyright 2026 Kangyou Choi
# SPDX-License-Identifier: Apache-2.0
#
# Checks (default) or adds (--fix) the SPDX license header on tracked source files.
#
#   scripts/license-headers.sh          # exit 1 and list files missing the header
#   scripts/license-headers.sh --fix    # insert the header into those files
set -euo pipefail

cd "$(dirname "$0")/.."

OWNER="Kangyou Choi"
YEAR="2026"
SPDX="SPDX-License-Identifier: Apache-2.0"

mode="check"
case "${1:-}" in
  "") ;;
  --fix) mode="fix" ;;
  *) echo "usage: $0 [--fix]" >&2; exit 2 ;;
esac

# Comment prefix for a path, or empty if the file type is not covered.
prefix_for() {
  case "$1" in
    *.rs | *.cu | *.metal | *.proto) echo "//" ;;
    *.sh | */Dockerfile | *.Dockerfile | Dockerfile) echo "#" ;;
    *) echo "" ;;
  esac
}

missing=()
while IFS= read -r -d '' file; do
  [ -n "$(prefix_for "$file")" ] || continue
  if ! head -n 6 "$file" | grep -qF "$SPDX"; then
    missing+=("$file")
  fi
done < <(git ls-files -z)

if [ "${#missing[@]}" -eq 0 ]; then
  echo "All source files carry the license header."
  exit 0
fi

if [ "$mode" = "check" ]; then
  echo "Missing license header (run scripts/license-headers.sh --fix):" >&2
  printf '  %s\n' "${missing[@]}" >&2
  exit 1
fi

for file in "${missing[@]}"; do
  p="$(prefix_for "$file")"
  header="$p Copyright $YEAR $OWNER"$'\n'"$p $SPDX"
  first="$(head -n 1 "$file")"
  tmp="$(mktemp)"
  # Keep a shebang or a Dockerfile parser directive on the first line.
  if [[ "$first" == '#!'* && "$first" != '#!['* ]] || [[ "$first" == '# syntax='* ]]; then
    { printf '%s\n' "$first"; printf '%s\n\n' "$header"; tail -n +2 "$file"; } > "$tmp"
  else
    { printf '%s\n\n' "$header"; cat "$file"; } > "$tmp"
  fi
  cat "$tmp" > "$file"   # preserves mode bits
  rm -f "$tmp"
done
echo "Added the license header to ${#missing[@]} files."
