
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(7);

SELECT ok(
	bare_callable_name('code+moniker://app/lang:ts/module:util/function:foo(number,string)'::moniker)
		= 'code+moniker://app/lang:ts/module:util/function:foo'::moniker,
	'TS typed signature stripped to bare name'
);

SELECT ok(
	bare_callable_name('code+moniker://app/lang:ts/module:util/function:foo(2)'::moniker)
		= 'code+moniker://app/lang:ts/module:util/function:foo'::moniker,
	'TS arity-only signature stripped to bare name'
);

SELECT ok(
	bare_callable_name('code+moniker://app/lang:ts/module:util/function:foo()'::moniker)
		= 'code+moniker://app/lang:ts/module:util/function:foo'::moniker,
	'TS empty parens stripped to bare name'
);

SELECT ok(
	bare_callable_name('code+moniker://app/lang:ts/module:util/class:Foo'::moniker)
		= 'code+moniker://app/lang:ts/module:util/class:Foo'::moniker,
	'last segment without parens is unchanged (no copy semantics)'
);

SELECT ok(
	bare_callable_name('code+moniker://app'::moniker)
		= 'code+moniker://app'::moniker,
	'project-only moniker is unchanged'
);

SELECT ok(
	bare_callable_name(
		'code+moniker://app/lang:ts/module:util/function:`f((x: number) => string)`'::moniker
	) = 'code+moniker://app/lang:ts/module:util/function:f'::moniker,
	'backtick-quoted typed signature stripped to bare name (consumer no longer needs to know about backticks)'
);

SELECT ok(
	bare_callable_name('code+moniker://app/lang:java/class:Plan/method:create(int,String)'::moniker)
		= 'code+moniker://app/lang:java/class:Plan/method:create'::moniker,
	'Java typed method signature stripped to bare name'
);

SELECT * FROM finish();

ROLLBACK;
