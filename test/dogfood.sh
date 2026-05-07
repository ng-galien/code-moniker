#!/usr/bin/env bash
# Ingest pg_code_moniker's own Rust source into a fresh DB via
# extract_rust(). After this script, query live in psql:
#
#   $HOME/.pgrx/17.9/pgrx-install/bin/psql -h localhost -p 28817 -d pcm_dogfood

set -euo pipefail

PG_BIN="${PG_BIN:-$HOME/.pgrx/17.9/pgrx-install/bin}"
PG_PORT="${PG_PORT:-28817}"
DB="${DB:-pcm_dogfood}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PSQL="$PG_BIN/psql -h localhost -p $PG_PORT -X -q -v ON_ERROR_STOP=1"

echo "== drop+create $DB"
$PSQL -d postgres -c "DROP DATABASE IF EXISTS $DB;" >/dev/null
$PSQL -d postgres -c "CREATE DATABASE $DB;" >/dev/null

echo "== install extension + module table"
$PSQL -d "$DB" <<SQL >/dev/null
CREATE EXTENSION pg_code_moniker;
CREATE TABLE module (
	source_uri text       PRIMARY KEY,
	graph      code_graph NOT NULL
);
SQL

echo "== ingest src/**/*.rs"
files=$(find "$ROOT/src" -name "*.rs" | sort)
count=0
for abs in $files; do
	rel="${abs#$ROOT/}"
	$PSQL -d "$DB" -c "INSERT INTO module(source_uri, graph)
		VALUES (
			'$rel',
			extract_rust('$rel', pg_read_file('$abs'), 'esac+moniker://pg_code_moniker'::moniker)
		);" >/dev/null
	count=$((count + 1))
done
echo "   ingested $count files"

echo "== indices for live queries (post-insert bulk-build)"
$PSQL -d "$DB" <<SQL >/dev/null
CREATE INDEX module_root_idx  ON module USING btree (graph_root(graph));
CREATE INDEX module_root_gist ON module USING gist  (graph_root(graph));
CREATE INDEX module_defs_gin  ON module USING gin   (graph_def_monikers(graph));
CREATE INDEX module_refs_gin  ON module USING gin   (graph_ref_targets(graph));
SQL

echo "== ready: $($PSQL -d "$DB" -A -t -c "SELECT count(*) FROM module") modules,
   $($PSQL -d "$DB" -A -t -c "SELECT sum(array_length(graph_def_monikers(graph),1)) FROM module") defs total,
   $($PSQL -d "$DB" -A -t -c "SELECT sum(array_length(graph_ref_targets(graph),1)) FROM module") refs total"
echo
echo "next: $PG_BIN/psql -h localhost -p $PG_PORT -d $DB"
