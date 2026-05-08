#!/usr/bin/env bash
# Ingest the dogfood panel into a fresh DB via extract_<lang>().
#
# Usage:
#   test/dogfood.sh                    # full panel
#   test/dogfood.sh --lang rs          # only Rust panel entries
#   test/dogfood.sh --only zod         # only one project_id
#   test/dogfood.sh --reset            # drop and re-clone all entries
#
# After this script, query live in psql:
#   $HOME/.pgrx/17.9/pgrx-install/bin/psql -h localhost -p 28817 -d pcm_dogfood
#
# Cloned repositories live at <repo_root>/dogfood/<lang>/<project_id>
# and are gitignored. Re-running without --reset reuses the existing
# clone without fetching.

set -euo pipefail

PG_BIN="${PG_BIN:-$HOME/.pgrx/17.9/pgrx-install/bin}"
PG_PORT="${PG_PORT:-28817}"
DB="${DB:-pcm_dogfood}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE="${CACHE:-$ROOT/dogfood}"
PSQL="$PG_BIN/psql -h localhost -p $PG_PORT -X -q -v ON_ERROR_STOP=1"

# shellcheck source=test/dogfood/panel.sh
source "$ROOT/test/dogfood/panel.sh"

LANG_FILTER=""
ONLY_FILTER=""
RESET=0

while [[ $# -gt 0 ]]; do
	case "$1" in
		--lang) LANG_FILTER="$2"; shift 2 ;;
		--only) ONLY_FILTER="$2"; shift 2 ;;
		--reset) RESET=1; shift ;;
		-h|--help) sed -n '2,15p' "$0"; exit 0 ;;
		*) echo "unknown flag: $1" >&2; exit 2 ;;
	esac
done

mkdir -p "$CACHE"

ensure_clone() {
	local lang="$1" project="$2" url="$3" ref="$4"
	local dest="$CACHE/$lang/$project"

	if [[ "$url" == "self" ]]; then
		echo "$ROOT"
		return 0
	fi

	if [[ "$RESET" -eq 1 && -d "$dest" ]]; then
		rm -rf "$dest"
	fi

	if [[ ! -d "$dest" ]]; then
		mkdir -p "$(dirname "$dest")"
		git clone --quiet --depth 1 --branch "$ref" "$url" "$dest" 2>/dev/null \
			|| git clone --quiet "$url" "$dest"
		git -C "$dest" checkout --quiet "$ref" 2>/dev/null || true
	fi

	echo "$dest"
}

ext_for_lang() {
	case "$1" in
		rs) echo "rs" ;;
		ts) echo "ts tsx js jsx mjs cjs mts cts" ;;
		java) echo "java" ;;
		sql) echo "sql psql pgsql" ;;
		py) echo "py pyi" ;;
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
		*) echo "" ;;
	esac
}

manifest_fn() {
	case "$1" in
		rs) echo "extract_cargo" ;;
		ts) echo "extract_package_json" ;;
		*) echo "" ;;
	esac
}

echo "== drop+create $DB"
$PSQL -d postgres -c "DROP DATABASE IF EXISTS $DB;" >/dev/null
$PSQL -d postgres -c "CREATE DATABASE $DB;" >/dev/null

echo "== install extension + schema"
$PSQL -d "$DB" <<SQL >/dev/null
CREATE EXTENSION pg_code_moniker;
CREATE TABLE module (
	project    text       NOT NULL,
	lang       text       NOT NULL,
	source_uri text       NOT NULL,
	graph      code_graph NOT NULL,
	PRIMARY KEY (project, source_uri)
);
CREATE TABLE package (
	project     text NOT NULL,
	name        text NOT NULL,
	version     text,
	dep_kind    text NOT NULL,
	import_root text NOT NULL,
	PRIMARY KEY (project, name, dep_kind)
);
SQL

ingest_one() {
	local lang="$1" project="$2" clone_path="$3" src_subdir="$4" manifest_path="$5"
	local fn extr exts
	extr="$(extract_fn "$lang")"
	exts="$(ext_for_lang "$lang")"
	if [[ -z "$extr" || -z "$exts" ]]; then
		echo "   ! unsupported lang=$lang for $project; skipped" >&2
		return 0
	fi

	# manifest ingestion
	if [[ "$manifest_path" != "-" ]]; then
		local manifest_abs="$clone_path/$manifest_path"
		if [[ -f "$manifest_abs" ]]; then
			fn="$(manifest_fn "$lang")"
			$PSQL -d "$DB" -c "
				INSERT INTO package(project, name, version, dep_kind, import_root)
				SELECT '$project', name, version, dep_kind, import_root
				FROM $fn(pg_read_file('$manifest_abs'))
				ON CONFLICT DO NOTHING;
			" >/dev/null
		fi
	fi

	# build the find expression for this language's extensions
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
	# `set -f` prevents shell glob expansion on the `*.<ext>` patterns.
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
		# Moniker URI is src_subdir-relative; source_uri keeps the full path.
		local logical="$rel"
		if [[ "$src_subdir" != "." && -n "$src_subdir" ]]; then
			logical="${rel#${src_subdir}/}"
		fi
		local source_escaped="${rel//\'/\'\'}"
		local logical_escaped="${logical//\'/\'\'}"
		printf "INSERT INTO module(project, lang, source_uri, graph) VALUES ('%s', '%s', '%s', %s('%s', pg_read_file('%s'), 'pcm+moniker://%s'::moniker));\n" \
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

summary=$($PSQL -d "$DB" -A -t -c "
SELECT
	project || E'\t' ||
	lang    || E'\t' ||
	count(*) || E'\t' ||
	coalesce(sum(array_length(graph_def_monikers(graph), 1)), 0) || E'\t' ||
	coalesce(sum(array_length(graph_ref_targets(graph), 1)), 0)
FROM module GROUP BY project, lang ORDER BY project;")

printf '\n== ready: project | lang | files | defs | refs\n'
printf '%s\n' "$summary"
echo
echo "next: $PG_BIN/psql -h localhost -p $PG_PORT -d $DB"
