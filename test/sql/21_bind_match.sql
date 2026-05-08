-- Phase 7 step 3: bind_match operator + GiST opclass entry +
-- end-to-end cross-file linkage.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(12);

-- Surface presence ---------------------------------------------------------

SELECT has_function('bind_match'::name, ARRAY['moniker','moniker'],
	'bind_match(moniker, moniker) is exposed');

-- Functional semantics -----------------------------------------------------

SELECT ok(
	bind_match(
		'esac+moniker://app/lang:python/module:util/path:Foo'::moniker,
		'esac+moniker://app/lang:python/module:util/class:Foo'::moniker),
	'path:Foo (import placeholder) bind_matches class:Foo (typed def)');

SELECT ok(
	bind_match(
		'esac+moniker://app/lang:python/module:util/function:Y()'::moniker,
		'esac+moniker://app/lang:python/module:util/function:Y(int,str)'::moniker) = false,
	'arity-only callable name does not bind_match a typed callable name (last-segment name byte-strict)');

SELECT ok(
	NOT bind_match(
		'esac+moniker://app1/lang:python/module:util/path:Foo'::moniker,
		'esac+moniker://app2/lang:python/module:util/class:Foo'::moniker),
	'cross-project bind_match never matches');

SELECT ok(
	NOT bind_match(
		'esac+moniker://app/lang:python/module:util/path:Foo'::moniker,
		'esac+moniker://app/lang:java/module:util/class:Foo'::moniker),
	'cross-language bind_match never matches');

SELECT ok(
	NOT bind_match(
		'esac+moniker://app/lang:python/module:util/path:Foo'::moniker,
		'esac+moniker://app/lang:python/module:util/class:Foo/method:bar(int)'::moniker),
	'different segment counts never match');

-- ?= operator surface ------------------------------------------------------

SELECT ok(
	'esac+moniker://app/lang:ts/path:src/path:lib/path:Lib'::moniker
		?= 'esac+moniker://app/lang:ts/path:src/path:lib/class:Lib'::moniker,
	'?= operator routes to bind_match');

-- GiST opclass: index supports the new strategy ---------------------------

CREATE TEMP TABLE m_idx (m moniker);
INSERT INTO m_idx VALUES
	('esac+moniker://app/lang:python/module:util/class:Foo'::moniker),
	('esac+moniker://app/lang:python/module:util/class:Bar'::moniker),
	('esac+moniker://app/lang:python/module:helpers/class:Foo'::moniker),
	('esac+moniker://app/lang:java/module:util/class:Foo'::moniker);

CREATE INDEX m_idx_gist ON m_idx USING gist (m);

SET LOCAL enable_seqscan = off;

SELECT is(
	(SELECT count(*)::int FROM m_idx
	  WHERE m ?= 'esac+moniker://app/lang:python/module:util/path:Foo'::moniker),
	1,
	'?= via GiST returns the single python util Foo def');

SELECT is(
	(SELECT count(*)::int FROM m_idx
	  WHERE m ?= 'esac+moniker://app/lang:python/module:util/path:Bar'::moniker),
	1,
	'?= via GiST returns the single python util Bar def');

SELECT is(
	(SELECT count(*)::int FROM m_idx
	  WHERE m ?= 'esac+moniker://app/lang:python/module:helpers/path:Foo'::moniker),
	1,
	'?= via GiST distinguishes helpers/Foo from util/Foo');

-- End-to-end cross-file linkage -------------------------------------------
--
-- Two python modules. `m_def` exports `class Foo`. `m_use` imports
-- `from m_def import Foo`. The import-side ref has a placeholder kind
-- on its last segment; bind_match unifies it with the export-side def.

CREATE TEMP TABLE module (
	id    text       PRIMARY KEY,
	graph code_graph NOT NULL
);

INSERT INTO module VALUES
	('m_def', extract_python(
		'm_def.py',
		E'class Foo:\n    pass\n',
		'esac+moniker://app'::moniker)),
	('m_use', extract_python(
		'm_use.py',
		E'from m_def import Foo\n',
		'esac+moniker://app'::moniker));

SELECT is(
	(SELECT count(*)::int FROM module m_use, LATERAL graph_refs(m_use.graph) r,
	                          module m_def, LATERAL graph_defs(m_def.graph) d
	   WHERE r.binding IN ('import', 'inject')
	     AND d.binding IN ('export', 'inject')
	     AND bind_match(r.target, d.moniker)
	     AND m_def.id = 'm_def'
	     AND m_use.id = 'm_use'),
	1,
	'cross-file linkage: 1 bind_match between import ref and export def');

-- Relative imports inside a package resolve against the importer's
-- module moniker (no `..` walk over the project root needed here).

INSERT INTO module VALUES
	('rel_def', extract_python(
		'acme/_models.py',
		E'class Response:\n    pass\n',
		'esac+moniker://app'::moniker)),
	('rel_use', extract_python(
		'acme/_client.py',
		E'from ._models import Response\n',
		'esac+moniker://app'::moniker));

SELECT is(
	(SELECT count(*)::int FROM module m_use, LATERAL graph_refs(m_use.graph) r,
	                          module m_def, LATERAL graph_defs(m_def.graph) d
	   WHERE r.binding = 'import'
	     AND d.binding = 'export'
	     AND bind_match(r.target, d.moniker)
	     AND m_def.id = 'rel_def'
	     AND m_use.id = 'rel_use'),
	1,
	'relative import (`from ._models import Response`) resolves via bind_match');

SELECT * FROM finish();

ROLLBACK;
