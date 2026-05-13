#!/usr/bin/env bash
# Run cargo check on Rust edits — catches type/borrow errors fast,
# without the full pgrx install cycle. Quiet on success.
set -euo pipefail

file_path=$(jq -r '.tool_input.file_path // empty' 2>/dev/null || true)
case "$file_path" in
	*.rs) ;;
	*) exit 0 ;;
esac

cd "${CLAUDE_PROJECT_DIR:-$(pwd)}"
cargo check --workspace --exclude code-moniker-pg --message-format=short 2>&1 | grep -E '^(error|warning)' | head -20 || true
