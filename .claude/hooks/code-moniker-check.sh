#!/usr/bin/env bash
set -euo pipefail

file_path=$(jq -r '.tool_input.file_path // empty' 2>/dev/null || true)
[ -n "$file_path" ] || exit 0
[ -f "$file_path" ] || exit 0

case "$file_path" in
	*/tests/fixtures/*) exit 0 ;;
esac

root="${CLAUDE_PROJECT_DIR:-$(pwd)}"
cd "$root"

set +e
output=$(cargo run --quiet -p code-moniker --bin code-moniker -- check "$file_path" 2>&1)
status=$?
set -e

if [ "$status" -ne 0 ]; then
	{
		echo "$output"
		if [ "$status" -eq 1 ]; then
			echo
			echo "code-moniker blocked this write. Fix every reported violation in this file."
		fi
	} >&2
	exit 2
fi
