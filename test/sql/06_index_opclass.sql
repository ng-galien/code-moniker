-- Btree and hash opclasses on `moniker`. Unlock ORDER BY, DISTINCT,
-- hash-join, and GIN on `moniker[]` (the SPEC `module_defs_gin` /
-- `module_refs_gin` patterns).

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(11);

-- Ordering operators present -----------------------------------------------

SELECT has_function('moniker_cmp'::name, ARRAY['moniker','moniker'],
	'moniker_cmp(moniker, moniker) is exposed');

SELECT has_function('moniker_hash'::name, ARRAY['moniker'],
	'moniker_hash(moniker) is exposed');

-- Btree behavior: total order with parents before children -----------------

SELECT is(
	(SELECT array_agg(m::text ORDER BY m)
	   FROM (VALUES
	     ('esac://app/c'::moniker),
	     ('esac://app/a'::moniker),
	     ('esac://app/b'::moniker)
	   ) AS t(m)),
	ARRAY['esac://app/a', 'esac://app/b', 'esac://app/c']::text[],
	'ORDER BY moniker uses the btree opclass');

SELECT ok(
	'esac://app/main'::moniker < 'esac://app/main/Foo'::moniker,
	'parent < child via btree');

SELECT ok(
	NOT ('esac://app/Foo'::moniker > 'esac://app/Foo'::moniker),
	'reflexive: moniker is not strictly greater than itself');

-- Hash behavior: DISTINCT works (hash-aggregate would otherwise fail) ----

SELECT is(
	(SELECT count(DISTINCT m)::int
	   FROM (VALUES
	     ('esac://app/a'::moniker),
	     ('esac://app/a'::moniker),
	     ('esac://app/b'::moniker)
	   ) AS t(m)),
	2,
	'DISTINCT moniker uses the hash opclass');

-- GIN array index on moniker[] -------------------------------------------

CREATE TEMP TABLE module (
	id    text       PRIMARY KEY,
	graph code_graph NOT NULL
);
CREATE INDEX module_defs_gin ON module USING gin (graph_def_monikers(graph));
CREATE INDEX module_refs_gin ON module USING gin (graph_ref_targets(graph));

-- GIN on `moniker[]` requires the moniker btree opclass via `array_ops`.
SELECT pass('GIN on graph_def_monikers(graph) created');
SELECT pass('GIN on graph_ref_targets(graph) created');

-- Insert two modules and round-trip the SPEC linkage pattern using the
-- array containment form (the one Phase 5 had to work around).

INSERT INTO module VALUES
	('lib', extract_typescript('src/lib.ts',
		'export class Lib { go() { return 1; } }',
		'esac://app'::moniker)),
	('app', extract_typescript('src/app.ts',
		'import { Lib } from "./lib";',
		'esac://app'::moniker));

SELECT is(
	(SELECT id FROM module
	  WHERE graph_def_monikers(graph) @> ARRAY['esac://app/src/lib#Lib#'::moniker]),
	'lib',
	'graph_def_monikers @> ARRAY[m] resolves the owning module');

SELECT is(
	(SELECT array_agg(id ORDER BY id) FROM module
	  WHERE graph_ref_targets(graph) @> ARRAY['esac://app/src/lib'::moniker]),
	ARRAY['app']::text[],
	'graph_ref_targets @> ARRAY[m] finds every importer');

-- ORDER BY a derived moniker expression — exercises btree on the result
-- of an extension function (graph_root).

SELECT is(
	(SELECT array_agg(graph_root(graph)::text ORDER BY graph_root(graph))
	   FROM module),
	ARRAY['esac://app/src/app', 'esac://app/src/lib']::text[],
	'ORDER BY on a moniker-returning expression');

SELECT * FROM finish();

ROLLBACK;
