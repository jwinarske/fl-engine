#!/usr/bin/env bash
# Runs a command; on failure, surfaces its last lines as error annotations.
# Exists for the same reason build.sh's die() annotates: raw logs live behind
# a blob host some working environments cannot reach, annotations ride the API.
set -uo pipefail
out="$(mktemp)"
"$@" 2>&1 | tee "$out"
rc=${PIPESTATUS[0]}
if [[ $rc -ne 0 ]]; then
  tail -n 8 "$out" | while IFS= read -r line; do
    [[ -n "$line" ]] && echo "::error title=$1::$line"
  done
fi
exit "$rc"
