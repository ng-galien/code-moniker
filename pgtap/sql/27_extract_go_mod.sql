
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(8);

SELECT has_function('extract_go_mod'::name, ARRAY['moniker', 'text'],
	'extract_go_mod(moniker, text) is exposed');

WITH parsed AS (
	SELECT * FROM extract_go_mod('code+moniker://app'::moniker, $t$
module github.com/foo/bar

go 1.21

require (
	github.com/x/y v1.2.3
	github.com/a/b v0.5.0 // indirect
)

require gopkg.in/x v1.0.0
$t$)
)
SELECT
	is((SELECT dep_kind FROM parsed WHERE name = 'github.com/foo/bar'),
		'package',
		'module declaration emits dep_kind=package') AS r1,
	is((SELECT version FROM parsed WHERE name = 'github.com/x/y'),
		'v1.2.3',
		'block-form require keeps version') AS r2,
	is((SELECT dep_kind FROM parsed WHERE name = 'github.com/a/b'),
		'indirect',
		'// indirect marker tags dep_kind=indirect') AS r3,
	is((SELECT version FROM parsed WHERE name = 'gopkg.in/x'),
		'v1.0.0',
		'single-line require parsed') AS r4,
	is((SELECT import_root FROM parsed WHERE name = 'github.com/x/y'),
		'github.com/x/y',
		'import_root preserves full Go module path') AS r5;


WITH parsed AS (
	SELECT * FROM extract_go_mod('code+moniker://app'::moniker, $t$
module foo

replace github.com/old => github.com/new v2.0.0

require github.com/x v1.0.0
$t$)
)
SELECT
	ok(NOT EXISTS (SELECT 1 FROM parsed WHERE name = 'github.com/old' OR name = 'github.com/new'),
		'replace directives are not emitted as deps') AS r6;


CREATE TEMP TABLE gomod(package_moniker moniker, name text, version text);
INSERT INTO gomod
	SELECT package_moniker, name, version
	FROM extract_go_mod('code+moniker://app'::moniker, $t$
module myapp

require github.com/gorilla/mux v1.8.0
$t$);

WITH g AS (
	SELECT extract_go(
		'cmd/main.go',
		E'package main\nimport "github.com/gorilla/mux"\nfunc Run() { mux.NewRouter() }\n',
		'code+moniker://app'::moniker
	) AS g
), ref_targets AS (
	SELECT t AS target
	FROM g, LATERAL unnest(graph_ref_targets(g)) t
)
SELECT
	ok((SELECT count(*)::int
		FROM ref_targets r
		JOIN gomod m ON m.package_moniker @> r.target) > 0,
		'package_moniker built from go.mod binds extractor ref targets via @>') AS r7;


SELECT * FROM finish();

ROLLBACK;
