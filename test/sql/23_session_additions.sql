
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(12);


SELECT is(
	('code+moniker://app/path:main'::moniker || 'class:Foo')::text,
	'code+moniker://app/path:main/class:Foo',
	'|| operator composes a typed child segment'
);

SELECT throws_ok(
	$$SELECT 'code+moniker://app/path:main'::moniker || 'no_kind_separator'$$,
	NULL,
	NULL,
	'|| rejects RHS that lacks the kind:name separator'
);


WITH g AS (
	SELECT graph_add_def(
		graph_create('code+moniker://app/path:m'::moniker, 'module'),
		'code+moniker://app/path:m/class:Foo'::moniker,
		'class',
		'code+moniker://app/path:m'::moniker,
		10,
		42
	) AS g
)
SELECT
	is((SELECT start_byte FROM graph_locate(g, 'code+moniker://app/path:m/class:Foo'::moniker)),
		10,
		'graph_locate returns the recorded start byte') AS r1,
	is((SELECT end_byte FROM graph_locate(g, 'code+moniker://app/path:m/class:Foo'::moniker)),
		42,
		'graph_locate returns the recorded end byte') AS r2,
	is((SELECT start_byte FROM graph_locate(g, 'code+moniker://app/path:m/class:Bar'::moniker)),
		NULL,
		'graph_locate returns NULL for monikers absent from the graph') AS r3
FROM g;


WITH g AS (
	SELECT graph_add_defs(
		graph_create('code+moniker://app/path:m'::moniker, 'module'),
		ARRAY['code+moniker://app/path:m/class:A',
		      'code+moniker://app/path:m/class:B']::moniker[],
		ARRAY['class','class']::text[],
		ARRAY['code+moniker://app/path:m','code+moniker://app/path:m']::moniker[]
	) AS g
)
SELECT is(
	(SELECT count(*)::int FROM graph_defs(g) WHERE kind = 'class'),
	2,
	'graph_add_defs inserts every row from parallel arrays in one call'
) FROM g;


SELECT ok(
	bind_match(
		'code+moniker://app/lang:sql/schema:esac/module:plan/function:create_plan(2)'::moniker,
		'code+moniker://app/lang:sql/schema:esac/module:plan/function:create_plan(uuid,text)'::moniker
	),
	'SQL refinement: arity-only ref name matches typed def by bare callable name'
);

SELECT ok(
	NOT bind_match(
		'code+moniker://app/lang:sql/schema:esac/module:plan/function:drop_plan(uuid)'::moniker,
		'code+moniker://app/lang:sql/schema:esac/module:plan/function:create_plan(uuid)'::moniker
	),
	'SQL refinement: distinct bare callable names do not match'
);

SELECT ok(
	bind_match(
		'code+moniker://app/lang:java/package:acme/class:Plan/method:create'::moniker,
		'code+moniker://app/lang:java/package:acme/class:Plan/method:create(int)'::moniker
	),
	'bare-name refinement applies to java: method:create bind_matches method:create(int)'
);


WITH p AS (
	SELECT * FROM extract_pyproject($$
		[project]
		name = "demo"
		version = "0.2.0"
		dependencies = ["httpx==0.27.2", "anyio>=3.7"]
		[project.optional-dependencies]
		test = ["pytest>=7.0"]
	$$)
)
SELECT
	is((SELECT version FROM p WHERE name = 'demo' AND dep_kind = 'package'),
		'0.2.0',
		'extract_pyproject yields the [project] package row') AS r1,
	is((SELECT version FROM p WHERE name = 'httpx'),
		'==0.27.2',
		'extract_pyproject keeps the version constraint operator') AS r2,
	is((SELECT dep_kind FROM p WHERE name = 'pytest'),
		'optional:test',
		'extract_pyproject prefixes optional groups with optional:<group>') AS r3
FROM p
LIMIT 1;


SELECT * FROM finish();

ROLLBACK;
