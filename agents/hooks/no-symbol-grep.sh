#!/bin/sh
# PreToolUse hook (Bash): recursive symbol greps over the source tree are the
# job of the symbolic index. Blocks grep/rg/ugrep -r of identifier-looking
# patterns targeting crates/ or src/ and points at the code-moniker queries.
input=$(cat)

command=$(printf '%s' "$input" | python3 -c '
import json, sys
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(0)
print(data.get("tool_input", {}).get("command", ""))
' 2>/dev/null) || exit 0

case "$command" in
	*grep*|*rg\ *|*ugrep*) ;;
	*) exit 0 ;;
esac

case "$command" in
	*" -r"*|*" -R"*|*--recursive*|*-rn*|*-rln*|*-rl\ *) ;;
	*) exit 0 ;;
esac

case "$command" in
	*crates/*|*src/*|*" ."*) ;;
	*) exit 0 ;;
esac

pattern=$(printf '%s' "$command" | grep -oE '"[A-Za-z_][A-Za-z0-9_:]{3,}"' | head -1)
[ -z "$pattern" ] && exit 0

cat >&2 << EOF
Recursive grep of a symbol over the source tree — use the symbolic index:
  code-moniker query 'symbol.search name:$pattern limit:10'
  code-moniker query 'symbol.usages uri:"<URI from search>"'
(grep stays fine for exact strings in files you already know: drop -r and name the file.)
EOF
exit 2
