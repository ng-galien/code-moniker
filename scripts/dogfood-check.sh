#!/usr/bin/env bash
# Assert that current extractor output meets the floors in
# scripts/dogfood/baselines.tsv. Catches silent regressions where a
# refactor swallows defs or refs that used to be emitted.
#
# Exit 0 = all floors met. Exit 1 = at least one floor violated. Exit 2
# = DB unreachable or baselines file missing.
#
# Run AFTER scripts/dogfood.sh has ingested the panel.

set -euo pipefail

PG_BIN="${PG_BIN:-$HOME/.pgrx/17.9/pgrx-install/bin}"
PG_PORT="${PG_PORT:-28817}"
DB="${DB:-pcm_dogfood}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TSV="$ROOT/scripts/dogfood/baselines.tsv"
PSQL="$PG_BIN/psql -h localhost -p $PG_PORT -d $DB -A -t -F '|' -v ON_ERROR_STOP=1"

if [[ ! -f "$TSV" ]]; then
	echo "error: baselines file missing: $TSV" >&2
	echo "       run scripts/dogfood-baseline.sh to generate it." >&2
	exit 2
fi

if ! $PG_BIN/psql -h localhost -p "$PG_PORT" -d "$DB" -A -t -c 'SELECT 1' >/dev/null 2>&1; then
	echo "error: cannot connect to $DB on port $PG_PORT. run scripts/dogfood.sh first." >&2
	exit 2
fi

actuals=$(mktemp)
trap 'rm -f "$actuals"' EXIT

$PSQL -c "
SELECT m.project || '|' || m.lang || '|def|' || d.kind || '|' || count(*)
FROM module m, graph_defs(m.graph) d
GROUP BY m.project, m.lang, d.kind
UNION ALL
SELECT m.project || '|' || m.lang || '|ref|' || r.kind || '|' || count(*)
FROM module m, graph_refs(m.graph) r
GROUP BY m.project, m.lang, r.kind
UNION ALL
SELECT m.project || '|' || m.lang || '|total|files|' || count(*)
FROM module m
GROUP BY m.project, m.lang
UNION ALL
SELECT m.project || '|' || m.lang || '|total|defs|' || coalesce(sum(array_length(graph_def_monikers(m.graph), 1)), 0)
FROM module m
GROUP BY m.project, m.lang
UNION ALL
SELECT m.project || '|' || m.lang || '|total|refs|' || coalesce(sum(array_length(graph_ref_targets(m.graph), 1)), 0)
FROM module m
GROUP BY m.project, m.lang;" >"$actuals"

violations=0
missing=0
while IFS='|' read -r project lang metric kind min; do
	[[ -z "$project" || "$project" == \#* ]] && continue
	prefix="${project}|${lang}|${metric}|${kind}|"
	actual_line=$(grep -F "$prefix" "$actuals" || true)
	if [[ -z "$actual_line" ]]; then
		printf '%-12s %-5s %-6s %-22s MISSING (expected min=%s)\n' \
			"$project" "$lang" "$metric" "$kind" "$min"
		missing=$((missing + 1))
		continue
	fi
	actual="${actual_line##*|}"
	if (( actual < min )); then
		printf '%-12s %-5s %-6s %-22s FAIL  actual=%s  min=%s\n' \
			"$project" "$lang" "$metric" "$kind" "$actual" "$min"
		violations=$((violations + 1))
	fi
done <"$TSV"

total=$(grep -cvE '^#' "$TSV" || true)
ok=$((total - violations - missing))
echo
echo "summary: $ok/$total floors met, $violations regressions, $missing missing"

if (( violations > 0 || missing > 0 )); then
	exit 1
fi
