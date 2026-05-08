
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(12);


SELECT has_function('graph_export_monikers'::name, ARRAY['code_graph'],
	'graph_export_monikers(code_graph) is exposed');

SELECT has_function('graph_import_targets'::name, ARRAY['code_graph'],
	'graph_import_targets(code_graph) is exposed');


WITH g AS (
	SELECT extract_python(
		'm.py',
		E'class Foo:\n    pass\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT binding FROM graph_defs(g) WHERE kind = 'class'),
		'export',
		'public class is binding=export') AS r1
FROM g;


WITH g AS (
	SELECT extract_python(
		'm.py',
		E'def _helper():\n    pass\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT binding FROM graph_defs(g) WHERE kind = 'function'),
		'local',
		'leading-underscore python function is binding=local') AS r2
FROM g;


WITH g AS (
	SELECT extract_python(
		'foo.py',
		E'class Foo:\n    def __secret(self):\n        pass\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT binding FROM graph_defs(g) WHERE kind = 'method'),
		'local',
		'visibility=private produces binding=local') AS r3
FROM g;


WITH g AS (
	SELECT extract_python(
		'm.py',
		E'def f(x):\n    return x\n',
		'pcm+moniker://app'::moniker,
		deep := true
	) AS g
)
SELECT
	is((SELECT binding FROM graph_defs(g) WHERE kind = 'param' LIMIT 1),
		'local',
		'param def is binding=local') AS r4
FROM g;


WITH g AS (
	SELECT extract_python(
		'm.py',
		'',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT binding FROM graph_defs(g) WHERE kind = 'module'),
		'export',
		'module root is binding=export') AS r5
FROM g;


WITH g AS (
	SELECT extract_python(
		'm.py',
		E'import json\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT binding FROM graph_refs(g) WHERE kind = 'imports_module'),
		'import',
		'imports_module is binding=import') AS r6
FROM g;


WITH g AS (
	SELECT extract_python(
		'foo.py',
		E'class Foo:\n    def m(self):\n        self.bar()\n    def bar(self):\n        pass\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT binding FROM graph_refs(g) WHERE kind = 'method_call'),
		'local',
		'method_call is binding=local') AS r7
FROM g;


WITH g AS (
	SELECT extract_typescript(
		'src/app.ts',
		E'register(FooImpl);',
		'pcm+moniker://app'::moniker,
		deep := false,
		di_register_callees := ARRAY['register']
	) AS g
)
SELECT
	is((SELECT binding FROM graph_refs(g) WHERE kind = 'di_register' LIMIT 1),
		'inject',
		'di_register is binding=inject') AS r8
FROM g;


WITH g AS (
	SELECT extract_python(
		'm.py',
		E'class Foo:\n    pass\n\ndef bar():\n    pass\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT array_length(graph_export_monikers(g), 1) FROM g),
		3,
		'graph_export_monikers includes module + Foo + bar') AS r9
FROM g;


WITH g AS (
	SELECT extract_python(
		'm.py',
		E'import json\nimport acme.util\n',
		'pcm+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT array_length(graph_import_targets(g), 1) FROM g),
		2,
		'graph_import_targets carries one entry per import ref') AS r10
FROM g;

SELECT * FROM finish();

ROLLBACK;
