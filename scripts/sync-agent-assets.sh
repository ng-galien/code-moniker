#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source_dir="$root/agents/skills/code-moniker"
target_dir="$root/crates/cli/assets/agent/code-moniker"

mkdir -p "$target_dir/references"
cp "$source_dir/SKILL.md" "$target_dir/SKILL.md"

for name in diagnose explore mcp query-dsl; do
	cp "$source_dir/references/$name.md" "$target_dir/references/$name.md"
done

printf '%s\n' "Synced code-moniker skill assets into crates/cli/assets/agent/code-moniker."
