#!/usr/bin/env sh
# PostToolUse guardrail: run the agent-profile check on the files the tool
# call touched. One script for every harness — pass the tool as $1 so the
# claude/codex variants can never drift again.
set -eu

tool="${1:?usage: code-moniker-agent.sh <claude|codex>}"
root="${CLAUDE_PROJECT_DIR:-${CODEX_PROJECT_DIR:-$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)}}"
cd "$root"

input_file=$(mktemp "${TMPDIR:-/tmp}/code-moniker-hook.XXXXXX")
trap 'rm -f "$input_file"' EXIT HUP INT TERM
cat > "$input_file"
files=$("$HOME/.cargo/bin/code-moniker" harness tool-files "$tool" "$input_file" 2>/dev/null) || {
	printf '%s\n' 'code-moniker hook could not inspect tool input' >&2
	exit 2
}

set -- '.'
while IFS= read -r file; do
	[ -n "$file" ] || continue
	[ -f "$file" ] || continue
	set -- "$@" --file "$file"
done <<CODE_MONIKER_FILES
$files
CODE_MONIKER_FILES

if [ "$#" -eq 1 ]; then
	exit 0
fi

if [ "$tool" = "codex" ]; then
	exec "$HOME/.cargo/bin/code-moniker" check --rules '.code-moniker.toml' --profile 'agent' --format codex-hook --max-violations 10 "$@"
fi

# claude: violations go to stderr and exit 2 so the harness blocks the edit.
set +e
output=$("$HOME/.cargo/bin/code-moniker" check --rules '.code-moniker.toml' --profile 'agent' --max-violations 10 "$@" 2>&1)
status=$?
set -e

if [ -n "$output" ]; then
	if [ "$status" -eq 0 ]; then
		printf '%s\n' "$output"
	else
		printf '%s\n' "$output" >&2
	fi
fi

if [ "$status" -eq 1 ]; then
	exit 2
fi

exit "$status"
