-- Python extraction smoke test.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(12);

SELECT has_function('extract_python'::name,
	ARRAY['text','text','moniker','boolean'],
	'extract_python(text, text, moniker, boolean) is exposed');

-- Module moniker: file path drives package: segments + module:basename.

WITH g AS (
	SELECT extract_python(
		'acme/util/text.py',
		'',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT is(graph_root(g)::text,
	'esac+moniker://app/package:acme/package:util/module:text',
	'file path drives the module moniker')
FROM g;

-- Top-level def with typed params: full signature in the moniker.

WITH g AS (
	SELECT extract_python(
		'm.py',
		E'def make(x: int, y: str) -> int:\n    return x\n',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/module:m/function:make(int,str)'::moniker,
		'function moniker carries full parameter type signature') AS r1,
	is((SELECT signature FROM graph_defs(g) WHERE kind = 'function'),
		'int,str',
		'function signature column lists parameter types') AS r2
FROM g;

-- Untyped slots collapse to the `_` placeholder.

WITH g AS (
	SELECT extract_python(
		'm.py',
		E'def f(a, b=1):\n    return a\n',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/module:m/function:f(_,_)'::moniker,
		'untyped python params collapse to `_` in the signature') AS r3
FROM g;

-- Method def excludes the implicit self from the signature, while
-- still being recorded as a method (containment-derived kind).

WITH g AS (
	SELECT extract_python(
		'foo.py',
		E'class Foo:\n    def bar(self, x: int) -> int:\n        return x\n',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac+moniker://app/module:foo/class:Foo/method:bar(int)'::moniker,
		'method moniker excludes self and uses kind=method') AS r4
FROM g;

-- Visibility convention: dunder=public, __x=private, _x=module, else public.

WITH g AS (
	SELECT extract_python(
		'm.py',
		E'def _helper():\n    pass\ndef __secret():\n    pass\ndef public_fn():\n    pass\n',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT visibility FROM graph_defs(g) d
	     WHERE kind = 'function' AND
	           moniker = 'esac+moniker://app/module:m/function:_helper()'::moniker),
		'module',
		'leading-underscore function is module-private') AS r5,
	is((SELECT visibility FROM graph_defs(g) d
	     WHERE kind = 'function' AND
	           moniker = 'esac+moniker://app/module:m/function:__secret()'::moniker),
		'private',
		'double-underscore (no trailing dunder) is private') AS r6,
	is((SELECT visibility FROM graph_defs(g) d
	     WHERE kind = 'function' AND
	           moniker = 'esac+moniker://app/module:m/function:public_fn()'::moniker),
		'public',
		'plain name is public') AS r7
FROM g;

-- Stdlib import gets confidence=external; project import gets imported.

WITH g AS (
	SELECT extract_python(
		'm.py',
		E'import json\nimport acme.util\n',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT confidence FROM graph_refs(g) r
	     WHERE r.kind = 'imports_module' AND r.target::text LIKE '%json%'),
		'external',
		'import json marked external') AS r8,
	is((SELECT confidence FROM graph_refs(g) r
	     WHERE r.kind = 'imports_module' AND r.target::text LIKE '%acme%'),
		'imported',
		'import acme.util marked imported') AS r9
FROM g;

-- Method call on self gets receiver_hint=self.

WITH g AS (
	SELECT extract_python(
		'foo.py',
		E'class Foo:\n    def m(self):\n        self.bar()\n    def bar(self):\n        pass\n',
		'esac+moniker://app'::moniker
	) AS g
)
SELECT
	is((SELECT receiver_hint FROM graph_refs(g) WHERE kind = 'method_call'),
		'self',
		'method_call on self carries receiver_hint=self') AS r10
FROM g;

SELECT * FROM finish();

ROLLBACK;
