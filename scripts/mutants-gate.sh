#!/usr/bin/env bash
set -euo pipefail

threshold="${1:-50}"
output_dir="${2:-target/mutants}"

set +e
cargo mutants --workspace --copy-target true --output "$output_dir"
mutants_status=$?
set -e

outcomes="$output_dir/mutants.out/outcomes.json"
if [[ ! -f "$outcomes" ]]; then
  echo "missing cargo-mutants outcomes file: $outcomes" >&2
  exit "$mutants_status"
fi

count_summary() {
  { grep -o "\"summary\": \"$1\"" "$outcomes" || true; } | wc -l | tr -d ' '
}

caught=$(count_summary "CaughtMutant")
missed=$(count_summary "MissedMutant")
timeout=$(count_summary "Timeout")
total=$((caught + missed + timeout))

if [[ "$total" -eq 0 ]]; then
  echo "no viable mutants were tested"
  exit 1
fi

score=$((caught * 100 / total))
echo "mutation score: ${score}% (${caught}/${total} caught)"

if [[ "$score" -lt "$threshold" ]]; then
  echo "mutation score ${score}% is below required ${threshold}%" >&2
  exit 1
fi
