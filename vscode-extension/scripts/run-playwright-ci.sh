#!/usr/bin/env bash

set -uo pipefail

if [[ $# -ne 1 ]]; then
	echo "Usage: $0 <npm-script>" >&2
	exit 2
fi

script_name=$1
extension_root=$(cd "$(dirname "$0")/.." && pwd)
results_dir="$extension_root/test-results"
screen_snapshot="$results_dir/runner-screen-latest.png"
mkdir -p "$results_dir"

(
	cd "$extension_root"
	npm run "$script_name"
) &
test_pid=$!

(
	while kill -0 "$test_pid" 2>/dev/null; do
		sleep 5
		scrot "$screen_snapshot" 2>/dev/null || true
	done
) &
snapshot_pid=$!

wait "$test_pid"
status=$?
kill "$snapshot_pid" 2>/dev/null || true
wait "$snapshot_pid" 2>/dev/null || true

if [[ $status -eq 0 ]]; then
	rm -f "$screen_snapshot"
else
	scrot "$results_dir/runner-screen-final.png" 2>/dev/null || true
fi

exit "$status"
