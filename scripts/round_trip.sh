#!/usr/bin/env bash
# Per-extractor round-trip + precise existence assertions on the dogfood
# panel. Run after `scripts/dogfood.sh`.
#
# For each panel project: assert that a hand-picked set of monikers (defs
# or refs) exists in the extracted graph, then run a declarative
# round-trip (graph → spec → graph) on one module and assert byte-equal.

set -euo pipefail

PG_BIN="${PG_BIN:-$HOME/.pgrx/17.9/pgrx-install/bin}"
PG_PORT="${PG_PORT:-28817}"
DB="${DB:-pcm_dogfood}"
PSQL="$PG_BIN/psql -h localhost -p $PG_PORT -X -q -A -t -v ON_ERROR_STOP=1 -d $DB"

pass=0
fail=0
report() {
	local status="$1" project="$2" check="$3" detail="$4"
	if [[ "$status" == "ok" ]]; then
		printf "  ok   %-12s %-40s %s\n" "$project" "$check" "$detail"
		pass=$((pass + 1))
	else
		printf "  FAIL %-12s %-40s %s\n" "$project" "$check" "$detail"
		fail=$((fail + 1))
	fi
}

assert_def_exists() {
	local project="$1" pattern="$2" label="$3"
	local n
	n=$($PSQL -c "
		SELECT count(*) FROM module
		WHERE project = '$project'
		  AND EXISTS (
		    SELECT 1 FROM unnest(graph_def_monikers(graph)) m
		    WHERE m::text LIKE '$pattern'
		  );
	")
	if [[ "$n" -ge 1 ]]; then
		report ok "$project" "def:$label" "found in $n module(s)"
	else
		report FAIL "$project" "def:$label" "no module matches pattern $pattern"
	fi
}

assert_ref_exists() {
	local project="$1" pattern="$2" label="$3"
	local n
	n=$($PSQL -c "
		SELECT count(*) FROM module
		WHERE project = '$project'
		  AND EXISTS (
		    SELECT 1 FROM unnest(graph_ref_targets(graph)) m
		    WHERE m::text LIKE '$pattern'
		  );
	")
	if [[ "$n" -ge 1 ]]; then
		report ok "$project" "ref:$label" "found in $n module(s)"
	else
		report FAIL "$project" "ref:$label" "no module ref-targets pattern $pattern"
	fi
}

assert_round_trip() {
	local project="$1" lang="$2"
	local result
	# By design, code_graph_to_spec preserves defs + canonical edges only
	# (imports_module | calls | di_register | di_require — others are walker
	# diagnostics dropped at spec time). The round-trip assertion checks
	# defs are byte-equal as multisets, root is byte-equal, and the count
	# of canonical refs in the source equals the count of refs in the
	# rebuilt graph.
	result=$($PSQL -c "
		WITH one AS (
		  SELECT graph FROM module
		  WHERE project = '$project'
		  ORDER BY array_length(graph_def_monikers(graph), 1) DESC NULLS LAST
		  LIMIT 1
		),
		spec AS (
		  SELECT code_graph_to_spec(graph) AS s FROM one
		),
		rebuilt AS (
		  SELECT code_graph_declare(s) AS g FROM spec
		),
		canonical_refs_before AS (
		  SELECT count(*) AS n FROM one, graph_refs(one.graph)
		  WHERE kind IN ('imports_module', 'calls', 'di_register', 'di_require')
		),
		refs_after AS (
		  SELECT count(*) AS n FROM rebuilt, graph_refs(rebuilt.g)
		),
		def_diff AS (
		  SELECT
		    (SELECT count(*) FROM one, unnest(graph_def_monikers(one.graph)) m1
		     WHERE NOT EXISTS (
		       SELECT 1 FROM rebuilt, unnest(graph_def_monikers(rebuilt.g)) m2 WHERE m1 = m2
		     ))
		    +
		    (SELECT count(*) FROM rebuilt, unnest(graph_def_monikers(rebuilt.g)) m2
		     WHERE NOT EXISTS (
		       SELECT 1 FROM one, unnest(graph_def_monikers(one.graph)) m1 WHERE m1 = m2
		     )) AS n
		)
		SELECT
		  array_length(graph_def_monikers(one.graph), 1)::text || '|' ||
		  array_length(graph_def_monikers(rebuilt.g), 1)::text || '|' ||
		  (SELECT n FROM canonical_refs_before)::text || '|' ||
		  (SELECT n FROM refs_after)::text || '|' ||
		  CASE WHEN graph_root(one.graph) = graph_root(rebuilt.g) THEN '1' ELSE '0' END || '|' ||
		  (SELECT n FROM def_diff)::text
		FROM one, rebuilt;
	")
	local d_before d_after r_before r_after root_eq diff
	IFS='|' read -r d_before d_after r_before r_after root_eq diff <<<"$result"
	if [[ "$root_eq" == "1" && "$d_before" == "$d_after" && "$r_before" == "$r_after" && "$diff" == "0" ]]; then
		report ok "$project" "round-trip ($lang)" "defs=$d_before  canon-refs=$r_before  root=eq  def-diff=0"
	else
		report FAIL "$project" "round-trip ($lang)" "defs $d_before→$d_after  canon-refs $r_before→$r_after  root_eq=$root_eq  def_diff=$diff"
	fi
}

echo "== precise def/ref assertions per panel project =="

# Rust — bytes
assert_def_exists bytes "%struct:Bytes" "struct:Bytes"
assert_def_exists bytes "%struct:BytesMut" "struct:BytesMut"

# Rust — clap
assert_def_exists clap "%trait:Parser" "trait:Parser"
assert_def_exists clap "%trait:Args" "trait:Args"
assert_def_exists clap "%struct:Command" "struct:Command"

# Rust — code-moniker (self)
assert_def_exists code-moniker "%struct:Walker" "struct:Walker"
assert_def_exists code-moniker "%struct:CodeGraph" "struct:CodeGraph"

# TypeScript — zod
assert_def_exists zod "%class:ZodString" "class:ZodString"
assert_def_exists zod "%class:ZodObject" "class:ZodObject"

# TypeScript — date-fns
assert_def_exists date-fns "%function:addDays%" "function:addDays"
assert_def_exists date-fns "%function:format%" "function:format(...)"

# Java — gson
assert_def_exists gson "%class:Gson" "class:Gson"
assert_def_exists gson "%class:GsonBuilder" "class:GsonBuilder"
assert_def_exists gson "%method:toJson%" "method:toJson(...)"

# Python — httpx
assert_def_exists httpx "%class:Client" "class:Client"
assert_def_exists httpx "%class:AsyncClient" "class:AsyncClient"

# Go — gorilla/mux
assert_def_exists mux "%struct:Router" "struct:Router"
assert_def_exists mux "%func:NewRouter%" "func:NewRouter()"

# C# — commandline
assert_def_exists commandline "%class:Parser" "class:Parser"

# SQL — pgtap (panel ingests upgrade scripts under sql/, not the main pgtap.sql.in)
assert_def_exists pgtap "%function:pgtap_version()" "function:pgtap_version()"
assert_def_exists pgtap "%function:has_tablespace%" "function:has_tablespace(...)"
assert_def_exists pgtap "%view:tap_funky" "view:tap_funky"

echo
echo "== declare → to_spec → declare round-trip per panel project =="

assert_round_trip bytes rs
assert_round_trip clap rs
assert_round_trip code-moniker rs
assert_round_trip zod ts
assert_round_trip date-fns ts
assert_round_trip gson java
assert_round_trip httpx py
assert_round_trip mux go
assert_round_trip commandline cs
assert_round_trip pgtap sql

echo
total=$((pass + fail))
echo "== summary: $pass / $total passed ($fail failed)"
exit $fail
