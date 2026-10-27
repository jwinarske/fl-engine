#!/usr/bin/env bash
# Runs a command; on failure, makes the failure readable through the REST API.
#
# Raw job logs redirect to a blob host that some working environments' proxies
# refuse. Two channels ride the API instead: annotations (capped at ten per
# step, so they carry the lines that name the failure -- panics, assertions,
# the test summary) and a check run whose output text holds the last 400
# lines of the step, for when ten lines are not enough. The check run needs
# `checks: write`; without a token the annotations still go out.
set -uo pipefail
label="$1"
out="$(mktemp)"
"$@" 2>&1 | tee "$out"
rc=${PIPESTATUS[0]}
if [[ $rc -ne 0 ]]; then
  # The lines that explain a failure, not the ones that precede it.
  grep -E "panicked at|assertion|^(left|right):|error(\[E[0-9]+\])?:|FAILED|failed:|^    [a-z_]+$" "$out" \
    | grep -v "^error: test failed, to rerun" | head -n 9 \
    | while IFS= read -r line; do echo "::error title=$label::$line"; done
  echo "::error title=$label::exit $rc; full tail in check run 'log: $label'"
  if [[ -n "${GITHUB_TOKEN:-}" && -n "${GITHUB_SHA:-}" ]]; then
    python3 - "$label" "$out" <<'PY'
import json, os, subprocess, sys
label, path = sys.argv[1], sys.argv[2]
text = open(path, errors="replace").read()
lines = text.splitlines()[-400:]
body = "\n".join(lines)[-60000:]
payload = {
    "name": f"log: {label}",
    "head_sha": os.environ["GITHUB_SHA"],
    "status": "completed",
    "conclusion": "failure",
    "output": {"title": f"{label} failed", "summary": f"last {len(lines)} lines", "text": "```\n" + body + "\n```"},
}
subprocess.run(
    ["gh", "api", f"repos/{os.environ['GITHUB_REPOSITORY']}/check-runs", "-X", "POST", "--input", "-"],
    input=json.dumps(payload).encode(), check=False,
    stdout=subprocess.DEVNULL, env={**os.environ, "GH_TOKEN": os.environ["GITHUB_TOKEN"]},
)
PY
  fi
fi
exit "$rc"
