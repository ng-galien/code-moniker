-- code_graph_declare(jsonb) → code_graph
--
-- Exercises the declarative constructor: ingest a JSON spec, validate
-- the produced graph carries declared defs, exposes the origin column,
-- and that declared defs link with extracted defs through bind_match.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(24);

SELECT has_function(
	'code_graph_declare'::name,
	ARRAY['jsonb'],
	'code_graph_declare(jsonb) is exposed'
);


-- 1) Minimal Java spec produces root + class def.
WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
		"lang": "java",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"visibility": "public"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
)
SELECT is(
	(SELECT count(*)::int FROM g, LATERAL graph_defs(g.g)),
	2,
	'minimal spec yields root + 1 class def'
) FROM g;


-- 2) Declared defs carry origin = 'declared'; root keeps origin = 'extracted'.
WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
		"lang": "java",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"visibility": "public"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
), defs AS (
	SELECT d.moniker, d.kind, d.origin
	FROM g, LATERAL graph_defs(g.g) d
)
SELECT is(
	(SELECT origin FROM defs WHERE kind = 'class'),
	'declared',
	'declared symbol has origin = declared'
);

WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
		"lang": "java",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"visibility": "public"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
), defs AS (
	SELECT d.moniker, d.kind, d.origin
	FROM g, LATERAL graph_defs(g.g) d
)
SELECT is(
	(SELECT origin FROM defs WHERE kind = 'module'),
	'extracted',
	'root def keeps origin = extracted'
);


-- 3) depends_on edge lowers to imports_module + binding=import.
WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
		"lang": "rs",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
				"visibility": "public"
			}
		],
		"edges": [
			{
				"from": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "depends_on",
				"to": "pcm+moniker://app/external_pkg:cargo/path:serde"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
), refs AS (
	SELECT r.kind, r.binding FROM g, LATERAL graph_refs(g.g) r
)
SELECT is(
	(SELECT kind FROM refs),
	'imports_module',
	'depends_on edge lowers to imports_module'
);

WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
		"lang": "rs",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
				"visibility": "public"
			}
		],
		"edges": [
			{
				"from": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "depends_on",
				"to": "pcm+moniker://app/external_pkg:cargo/path:serde"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
), refs AS (
	SELECT r.kind, r.binding FROM g, LATERAL graph_refs(g.g) r
)
SELECT is(
	(SELECT binding FROM refs),
	'import',
	'depends_on edge gets binding = import'
);


-- 4) injects:require lowers to di_require + binding=inject.
WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
		"lang": "rs",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
				"visibility": "public"
			}
		],
		"edges": [
			{
				"from": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "injects:require",
				"to": "pcm+moniker://app/srcset:main/lang:rs/module:other/trait:Repo"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
), refs AS (
	SELECT r.kind, r.binding FROM g, LATERAL graph_refs(g.g) r
)
SELECT is(
	(SELECT kind FROM refs),
	'di_require',
	'injects:require edge lowers to di_require'
);

WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
		"lang": "rs",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
				"visibility": "public"
			}
		],
		"edges": [
			{
				"from": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "injects:require",
				"to": "pcm+moniker://app/srcset:main/lang:rs/module:other/trait:Repo"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
), refs AS (
	SELECT r.kind, r.binding FROM g, LATERAL graph_refs(g.g) r
)
SELECT is(
	(SELECT binding FROM refs),
	'inject',
	'injects:require edge gets binding = inject'
);


-- 5) Validation errors surface from the constructor.
SELECT throws_like(
	$$ SELECT code_graph_declare('{"root":"pcm+moniker://app/srcset:m/lang:cobol/module:f","lang":"cobol","symbols":[]}'::jsonb) $$,
	'%unknown lang%cobol%',
	'unknown lang is rejected'
);

SELECT throws_like(
	$$ SELECT code_graph_declare(
		'{"root":"pcm+moniker://app/srcset:m/lang:java/module:F","lang":"java","symbols":[
			{"moniker":"pcm+moniker://app/srcset:m/lang:java/module:F/trait:T","kind":"trait","parent":"pcm+moniker://app/srcset:m/lang:java/module:F"}
		]}'::jsonb) $$,
	'%trait%not allowed for lang=java%',
	'kind outside lang profile is rejected'
);

SELECT throws_like(
	$$ SELECT code_graph_declare(
		'{"root":"pcm+moniker://app/srcset:m/lang:java/module:F","lang":"java","symbols":[
			{"moniker":"pcm+moniker://app/srcset:m/lang:java/module:F/class:Foo","kind":"interface","parent":"pcm+moniker://app/srcset:m/lang:java/module:F"}
		]}'::jsonb) $$,
	'%does not match the moniker%',
	'kind mismatch with moniker last segment is rejected'
);

SELECT throws_like(
	$$ SELECT code_graph_declare(
		'{"root":"pcm+moniker://app/srcset:m/lang:java/module:F","lang":"java","symbols":[
			{"moniker":"pcm+moniker://app/srcset:m/lang:java/module:F/class:X","kind":"class","parent":"pcm+moniker://app/srcset:m/lang:java/module:F"},
			{"moniker":"pcm+moniker://app/srcset:m/lang:java/module:F/class:X","kind":"class","parent":"pcm+moniker://app/srcset:m/lang:java/module:F"}
		]}'::jsonb) $$,
	'%duplicate moniker%',
	'duplicate moniker is rejected'
);


-- 6) graph_export_monikers includes declared exports (declared symbols
-- with public visibility get binding=export by default).
WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
		"lang": "java",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"visibility": "public"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
)
SELECT is(
	(SELECT count(*)::int FROM g, LATERAL unnest(graph_export_monikers(g.g))),
	2,
	'declared exports surface via graph_export_monikers (root + class)'
);


-- 7) bind_match across two graphs: a declared def and an extracted-shaped
-- def with the same moniker resolve identically through bind_match.
WITH declared_g AS (
	SELECT code_graph_declare('{
		"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
		"lang": "java",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"visibility": "public"
			}
		]
	}'::jsonb) AS g
), extracted_g AS (
	SELECT graph_add_def(
		graph_create(
			'pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo'::moniker,
			'module'),
		'pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo'::moniker,
		'class',
		'pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo'::moniker
	) AS g
)
SELECT ok(
	bind_match(d.moniker, e.moniker),
	'declared and extracted defs with same moniker bind_match'
)
FROM
	(SELECT moniker FROM declared_g, LATERAL graph_defs(declared_g.g) d2 WHERE d2.kind = 'class') d,
	(SELECT moniker FROM extracted_g, LATERAL graph_defs(extracted_g.g) e2 WHERE e2.kind = 'class') e;


-- 8) calls intra-module gets binding=local.
WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
		"lang": "rs",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "pcm+moniker://app/srcset:main/lang:rs/module:svc"
			},
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:g()",
				"kind": "fn",
				"parent": "pcm+moniker://app/srcset:main/lang:rs/module:svc"
			}
		],
		"edges": [
			{
				"from": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "calls",
				"to":   "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:g()"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
), refs AS (
	SELECT r.kind, r.binding FROM g, LATERAL graph_refs(g.g) r
)
SELECT is(
	(SELECT binding FROM refs),
	'local',
	'intra-module calls edge gets binding = local'
);


-- 9) calls cross-module gets binding=none.
WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
		"lang": "rs",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "fn",
				"parent": "pcm+moniker://app/srcset:main/lang:rs/module:svc"
			}
		],
		"edges": [
			{
				"from": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
				"kind": "calls",
				"to":   "pcm+moniker://app/srcset:main/lang:rs/module:other/fn:g()"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
), refs AS (
	SELECT r.binding FROM g, LATERAL graph_refs(g.g) r
)
SELECT is(
	(SELECT binding FROM refs),
	'none',
	'cross-module calls edge gets binding = none'
);


-- 10) Topological sort: parent declared after child still works.
WITH spec AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
		"lang": "java",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo/method:bar()",
				"kind": "method",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo"
			},
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"visibility": "public"
			}
		]
	}'::jsonb AS s
), g AS (
	SELECT code_graph_declare(s) AS g FROM spec
)
SELECT is(
	(SELECT count(*)::int FROM g, LATERAL graph_defs(g.g)),
	3,
	'symbols out of topological order are reordered before insert'
);


-- =================================================================
-- code_graph_to_spec : reverse projection
-- =================================================================

SELECT has_function(
	'code_graph_to_spec'::name,
	ARRAY['code_graph'],
	'code_graph_to_spec(code_graph) is exposed'
);


-- 11) Round-trip declare → to_spec → declare yields equivalent graph.
WITH input AS (
	SELECT '{
		"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
		"lang": "java",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"visibility": "public"
			},
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo/method:bar()",
				"kind": "method",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"visibility": "public"
			}
		],
		"edges": [{
			"from": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo/method:bar()",
			"kind": "calls",
			"to":   "pcm+moniker://app/srcset:main/lang:java/package:com/module:Other/class:Other/method:baz()"
		}]
	}'::jsonb AS s
), g1 AS (
	SELECT code_graph_declare(s) AS g FROM input
), spec1 AS (
	SELECT code_graph_to_spec(g) AS s FROM g1
), g2 AS (
	SELECT code_graph_declare(s) AS g FROM spec1
), spec2 AS (
	SELECT code_graph_to_spec(g) AS s FROM g2
)
SELECT is(
	(SELECT s FROM spec1),
	(SELECT s FROM spec2),
	'declare → to_spec → declare → to_spec is idempotent'
);


-- 12) lang field is inferred from root's lang: segment.
WITH g AS (
	SELECT code_graph_declare('{
		"root": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
		"lang": "rs",
		"symbols": []
	}'::jsonb) AS g
)
SELECT is(
	(SELECT code_graph_to_spec(g)->>'lang' FROM g),
	'rs',
	'spec.lang is recovered from root lang: segment'
);


-- 13) Symbols emitted = number of non-root defs.
WITH g AS (
	SELECT code_graph_declare('{
		"root": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
		"lang": "java",
		"symbols": [
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"kind": "class",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo",
				"visibility": "public"
			},
			{
				"moniker": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo/method:bar()",
				"kind": "method",
				"parent": "pcm+moniker://app/srcset:main/lang:java/package:com/module:Foo/class:Foo",
				"visibility": "public"
			}
		]
	}'::jsonb) AS g
)
SELECT is(
	(SELECT jsonb_array_length(code_graph_to_spec(g)->'symbols') FROM g),
	2,
	'symbols array carries every non-root def'
);


-- 14) Canonical edges preserved in to_spec output.
WITH g AS (
	SELECT code_graph_declare('{
		"root": "pcm+moniker://app/srcset:main/lang:rs/module:svc",
		"lang": "rs",
		"symbols": [{
			"moniker": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
			"kind": "fn",
			"parent": "pcm+moniker://app/srcset:main/lang:rs/module:svc"
		}],
		"edges": [{
			"from": "pcm+moniker://app/srcset:main/lang:rs/module:svc/fn:f()",
			"kind": "depends_on",
			"to":   "pcm+moniker://app/external_pkg:cargo/path:serde"
		}]
	}'::jsonb) AS g
)
SELECT is(
	(SELECT (code_graph_to_spec(g)->'edges'->0->>'kind') FROM g),
	'depends_on',
	'depends_on edge survives the round-trip lift'
);


-- 15) Non-canonical ref kinds are dropped (graph_add_ref with extends).
WITH g AS (
	SELECT graph_add_ref(
		graph_add_def(
			graph_create(
				'pcm+moniker://app/srcset:main/lang:rs/module:svc'::moniker,
				'module'),
			'pcm+moniker://app/srcset:main/lang:rs/module:svc/struct:S'::moniker,
			'struct',
			'pcm+moniker://app/srcset:main/lang:rs/module:svc'::moniker
		),
		'pcm+moniker://app/srcset:main/lang:rs/module:svc/struct:S'::moniker,
		'pcm+moniker://app/srcset:main/lang:rs/module:other/trait:T'::moniker,
		'extends'
	) AS g
)
SELECT is(
	(SELECT jsonb_array_length(code_graph_to_spec(g)->'edges') FROM g),
	0,
	'extends ref is silently dropped from the spec'
);


-- 16) Errors when root has no lang: segment.
SELECT throws_like(
	$$ SELECT code_graph_to_spec(
		graph_create('pcm+moniker://app/srcset:main'::moniker, 'srcset')
	) $$,
	'%has no `lang:` segment%',
	'root without lang: segment is rejected'
);


SELECT * FROM finish();
ROLLBACK;
