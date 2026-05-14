#!/usr/bin/env bash
# Dogfood pipeline: ingest the panel into pcm_dogfood, snapshot the
# resulting counts as regression floors, or assert current counts meet
# those floors.
#
# Usage:
#   scripts/dogfood/run.sh ingest [--lang rs] [--only zod] [--reset]
#   scripts/dogfood/run.sh baseline                              # FLOOR_RATIO=0.95 by default
#   scripts/dogfood/run.sh check                                 # exits 1 on any regression
#
# Cloned projects land in <repo_root>/dogfood/<lang>/<project>/ and are
# gitignored. Re-running `ingest` reuses the existing clone unless
# --reset is passed.

set -euo pipefail

PG_BIN="${PG_BIN:-$HOME/.pgrx/17.9/pgrx-install/bin}"
PG_PORT="${PG_PORT:-28817}"
DB="${DB:-pcm_dogfood}"
FLOOR_RATIO="${FLOOR_RATIO:-0.95}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CACHE="${CACHE:-$ROOT/dogfood}"
TSV="$ROOT/scripts/dogfood/baselines.tsv"
PSQL="$PG_BIN/psql -h localhost -p $PG_PORT -X -q -v ON_ERROR_STOP=1"

# shellcheck source=panel.sh
source "$ROOT/scripts/dogfood/panel.sh"

require_db() {
	if ! $PG_BIN/psql -h localhost -p "$PG_PORT" -d "$DB" -A -t -c 'SELECT 1' >/dev/null 2>&1; then
		echo "error: cannot connect to $DB on port $PG_PORT. run 'scripts/dogfood/run.sh ingest' first." >&2
		exit 2
	fi
}

# Per-(project, lang, metric, kind) counts. Used as a subquery in
# `baseline` and as a top-level statement in `check` — no trailing
# semicolon in the heredoc.
counts_sql() {
	cat <<'SQL'
SELECT m.project, m.lang, 'def', d.kind, count(*)
FROM module m, graph_defs(m.graph) d
GROUP BY m.project, m.lang, d.kind
UNION ALL
SELECT m.project, m.lang, 'ref', r.kind, count(*)
FROM module m, graph_refs(m.graph) r
GROUP BY m.project, m.lang, r.kind
UNION ALL
SELECT m.project, m.lang, 'total', 'files', count(*)
FROM module m
GROUP BY m.project, m.lang
UNION ALL
SELECT m.project, m.lang, 'total', 'defs',
       coalesce(sum(array_length(graph_def_monikers(m.graph), 1)), 0)
FROM module m
GROUP BY m.project, m.lang
UNION ALL
SELECT m.project, m.lang, 'total', 'refs',
       coalesce(sum(array_length(graph_ref_targets(m.graph), 1)), 0)
FROM module m
GROUP BY m.project, m.lang
SQL
}

ext_for_lang() {
	case "$1" in
		rs) echo "rs" ;;
		ts) echo "ts tsx js jsx mjs cjs mts cts" ;;
		java) echo "java" ;;
		sql) echo "sql psql pgsql" ;;
		py) echo "py pyi" ;;
		go) echo "go" ;;
		cs) echo "cs" ;;
		*) echo "" ;;
	esac
}

extract_fn() {
	case "$1" in
		rs) echo "extract_rust" ;;
		ts) echo "extract_typescript" ;;
		java) echo "extract_java" ;;
		sql) echo "extract_plpgsql" ;;
		py) echo "extract_python" ;;
		go) echo "extract_go" ;;
		cs) echo "extract_csharp" ;;
		*) echo "" ;;
	esac
}

manifest_fn() {
	case "$1" in
		rs) echo "extract_cargo" ;;
		ts) echo "extract_package_json" ;;
		go) echo "extract_go_mod" ;;
		cs) echo "extract_csproj" ;;
		*) echo "" ;;
	esac
}

cmd_ingest() {
	local LANG_FILTER="" ONLY_FILTER="" RESET=0
	while [[ $# -gt 0 ]]; do
		case "$1" in
			--lang) LANG_FILTER="$2"; shift 2 ;;
			--only) ONLY_FILTER="$2"; shift 2 ;;
			--reset) RESET=1; shift ;;
			*) echo "unknown flag: $1" >&2; exit 2 ;;
		esac
	done

	mkdir -p "$CACHE"

	ensure_clone() {
		local lang="$1" project="$2" url="$3" ref="$4"
		local dest="$CACHE/$lang/$project"
		if [[ "$url" == "self" ]]; then echo "$ROOT"; return 0; fi
		if [[ "$RESET" -eq 1 && -d "$dest" ]]; then rm -rf "$dest"; fi
		if [[ ! -d "$dest" ]]; then
			mkdir -p "$(dirname "$dest")"
			git clone --quiet --depth 1 --branch "$ref" "$url" "$dest" 2>/dev/null \
				|| git clone --quiet "$url" "$dest"
			git -C "$dest" checkout --quiet "$ref" 2>/dev/null || true
		fi
		echo "$dest"
	}

	echo "== drop+create $DB"
	$PSQL -d postgres -c "DROP DATABASE IF EXISTS $DB;" >/dev/null
	$PSQL -d postgres -c "CREATE DATABASE $DB;" >/dev/null

	echo "== install extension + schema"
	$PSQL -d "$DB" <<SQL >/dev/null
CREATE EXTENSION code_moniker;
SET search_path = code_moniker, public;
CREATE TABLE module (
	project    text       NOT NULL,
	lang       text       NOT NULL,
	source_uri text       NOT NULL,
	graph      code_graph NOT NULL,
	PRIMARY KEY (project, source_uri)
);
CREATE TABLE package (
	project         text    NOT NULL,
	package_moniker moniker NOT NULL,
	name            text    NOT NULL,
	version         text,
	dep_kind        text    NOT NULL,
	import_root     text    NOT NULL,
	PRIMARY KEY (project, name, dep_kind)
);
SQL

	$PSQL -d postgres -c "ALTER DATABASE $DB SET search_path = code_moniker, public;" >/dev/null

	ingest_one() {
		local lang="$1" project="$2" clone_path="$3" src_subdir="$4" manifest_path="$5"
		local fn extr exts
		extr="$(extract_fn "$lang")"
		exts="$(ext_for_lang "$lang")"
		if [[ -z "$extr" || -z "$exts" ]]; then
			echo "   ! unsupported lang=$lang for $project; skipped" >&2
			return 0
		fi

		if [[ "$manifest_path" != "-" ]]; then
			local manifest_abs="$clone_path/$manifest_path"
			if [[ -f "$manifest_abs" ]]; then
				fn="$(manifest_fn "$lang")"
				$PSQL -d "$DB" -c "
					INSERT INTO package(project, package_moniker, name, version, dep_kind, import_root)
					SELECT '$project', package_moniker, name, version, dep_kind, import_root
					FROM $fn('code+moniker://$project'::moniker, pg_read_file('$manifest_abs'))
					ON CONFLICT DO NOTHING;
				" >/dev/null
			fi
		fi

		local find_expr=()
		local first=1
		for ext in $exts; do
			if [[ "$first" -eq 1 ]]; then
				find_expr+=( -name "*.$ext" )
				first=0
			else
				find_expr+=( -o -name "*.$ext" )
			fi
		done

		local src_root="$clone_path/$src_subdir"
		if [[ ! -d "$src_root" ]]; then
			echo "   ! src_subdir=$src_subdir missing under $clone_path; skipped" >&2
			return 0
		fi

		local files
		set -f
		files=$(find "$src_root" -type f \( "${find_expr[@]}" \) | sort)
		set +f

		local count=0
		local batch_sql
		batch_sql=$(mktemp)
		echo "BEGIN;" >"$batch_sql"
		while IFS= read -r abs; do
			[[ -z "$abs" ]] && continue
			local rel="${abs#$clone_path/}"
			local logical="$rel"
			if [[ "$src_subdir" != "." && -n "$src_subdir" ]]; then
				logical="${rel#${src_subdir}/}"
			fi
			local source_escaped="${rel//\'/\'\'}"
			local logical_escaped="${logical//\'/\'\'}"
			printf "INSERT INTO module(project, lang, source_uri, graph) VALUES ('%s', '%s', '%s', %s('%s', pg_read_file('%s'), 'code+moniker://%s'::moniker));\n" \
				"$project" "$lang" "$source_escaped" "$extr" "$logical_escaped" "$abs" "$project" \
				>>"$batch_sql"
			count=$((count + 1))
		done <<<"$files"
		echo "COMMIT;" >>"$batch_sql"
		$PSQL -d "$DB" -f "$batch_sql" >/dev/null
		rm -f "$batch_sql"
		echo "   $project: $count files ingested"
	}

	for entry in "${PCM_DOGFOOD_PANEL[@]}"; do
		IFS='|' read -r lang project url ref src_subdir manifest_path <<<"$entry"
		[[ -n "$LANG_FILTER" && "$lang" != "$LANG_FILTER" ]] && continue
		[[ -n "$ONLY_FILTER" && "$project" != "$ONLY_FILTER" ]] && continue
		echo "== $lang/$project ($url@$ref)"
		clone_path="$(ensure_clone "$lang" "$project" "$url" "$ref")"
		ingest_one "$lang" "$project" "$clone_path" "$src_subdir" "$manifest_path"
	done

	echo "== indices for live queries (post-insert bulk-build)"
	$PSQL -d "$DB" <<SQL >/dev/null
CREATE INDEX module_root_idx  ON module USING btree (graph_root(graph));
CREATE INDEX module_root_gist ON module USING gist  (graph_root(graph));
CREATE INDEX module_defs_gin  ON module USING gin   (graph_def_monikers(graph));
CREATE INDEX module_refs_gin  ON module USING gin   (graph_ref_targets(graph));
SQL

	local summary
	summary=$($PSQL -d "$DB" -A -t -c "
SELECT project || E'\t' || lang || E'\t' || count(*) || E'\t' ||
       coalesce(sum(array_length(graph_def_monikers(graph), 1)), 0) || E'\t' ||
       coalesce(sum(array_length(graph_ref_targets(graph), 1)), 0)
FROM module GROUP BY project, lang ORDER BY project;")

	printf '\n== ready: project | lang | files | defs | refs\n'
	printf '%s\n' "$summary"
	echo
	echo "next: $PG_BIN/psql -h localhost -p $PG_PORT -d $DB"
}

cmd_baseline() {
	require_db
	local tmp
	tmp=$(mktemp)

	{
		echo "# Regression floors for the dogfood panel. Generated by scripts/dogfood/run.sh baseline."
		echo "# Format: project|lang|metric|kind|min"
		echo "# - metric=def       kind = a def kind (method, comment, struct, ...)"
		echo "# - metric=ref       kind = a ref kind (calls, imports_module, ...)"
		echo "# - metric=total     kind ∈ {files, defs, refs}"
		echo "# Floors are floor(actual * ${FLOOR_RATIO}). Regenerate after intentional behavior changes."
		echo "#"
	} >"$tmp"

	$PSQL -d "$DB" -A -t -F '|' -c "
SELECT project, lang, metric, kind, GREATEST(1, floor(n * ${FLOOR_RATIO})::bigint)
FROM ($(counts_sql)) c(project, lang, metric, kind, n)
ORDER BY project, metric, kind;
" >>"$tmp"

	mkdir -p "$(dirname "$TSV")"
	mv "$tmp" "$TSV"

	local lines
	lines=$(grep -cvE '^#' "$TSV" || true)
	echo "wrote $TSV ($lines floors, ratio=$FLOOR_RATIO)"
}

cmd_check() {
	if [[ ! -f "$TSV" ]]; then
		echo "error: baselines file missing: $TSV" >&2
		echo "       run 'scripts/dogfood/run.sh baseline' to generate it." >&2
		exit 2
	fi
	require_db

	local actuals
	actuals=$(mktemp)

	$PSQL -d "$DB" -A -t -F '|' -c "$(counts_sql);" >"$actuals"

	local violations=0 missing=0
	while IFS='|' read -r project lang metric kind min; do
		[[ -z "$project" || "$project" == \#* ]] && continue
		local prefix="${project}|${lang}|${metric}|${kind}|"
		local actual_line
		actual_line=$(grep -F "$prefix" "$actuals" || true)
		if [[ -z "$actual_line" ]]; then
			printf '%-12s %-5s %-6s %-22s MISSING (expected min=%s)\n' \
				"$project" "$lang" "$metric" "$kind" "$min"
			missing=$((missing + 1))
			continue
		fi
		local actual="${actual_line##*|}"
		if (( actual < min )); then
			printf '%-12s %-5s %-6s %-22s FAIL  actual=%s  min=%s\n' \
				"$project" "$lang" "$metric" "$kind" "$actual" "$min"
			violations=$((violations + 1))
		fi
	done <"$TSV"

	local total ok
	total=$(grep -cvE '^#' "$TSV" || true)
	ok=$((total - violations - missing))
	echo
	echo "summary: $ok/$total floors met, $violations regressions, $missing missing"

	rm -f "$actuals"
	if (( violations > 0 || missing > 0 )); then
		exit 1
	fi
}

case "${1:-}" in
	ingest) shift; cmd_ingest "$@" ;;
	baseline) shift; cmd_baseline "$@" ;;
	check) shift; cmd_check "$@" ;;
	-h|--help|"") sed -n '2,${/^[^#]/q;p;}' "$0"; exit 0 ;;
	*) echo "unknown subcommand: $1" >&2; sed -n '2,${/^[^#]/q;p;}' "$0"; exit 2 ;;
esac
