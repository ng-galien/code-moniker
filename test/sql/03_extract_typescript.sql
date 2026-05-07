-- Phase 3: TypeScript extraction. End-to-end: source text in,
-- code_graph out with the right defs and root.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(8);

-- Surface presence ----------------------------------------------------------

SELECT has_function('extract_typescript'::name,
	ARRAY['text','text','moniker'],
	'extract_typescript(text, text, moniker) is exposed');

-- Extracting an empty source still produces a graph rooted at the module.

WITH empty AS (
	SELECT extract_typescript(
		'util.ts',
		'',
		'esac://app/main'::moniker
	) AS g
)
SELECT
	is(graph_root(g)::text, 'esac://app/main/util',
		'module moniker = anchor + file basename (extension stripped)') AS r1,
	is(array_length(graph_def_monikers(g), 1), 1,
		'empty source yields a graph with the module def only') AS r2
FROM empty;

-- Class with a method emits the class and the method as defs.

WITH g AS (
	SELECT extract_typescript(
		'src/Foo.ts',
		'export class Foo { bar() { return 1; } }',
		'esac://app/main'::moniker
	) AS g
)
SELECT
	ok(g @> 'esac://app/main/src/Foo'::moniker,
		'graph contains the module moniker') AS r3,
	ok(g @> 'esac://app/main/src/Foo#Foo#'::moniker,
		'graph contains the class def') AS r4,
	ok(g @> 'esac://app/main/src/Foo#Foo#bar().'::moniker,
		'graph contains the method def') AS r5
FROM g;

-- Imports emit refs from the module to the imported path.

WITH g AS (
	SELECT extract_typescript(
		'src/index.ts',
		'import { foo } from "./util";',
		'esac://app/main'::moniker
	) AS g
)
SELECT
	is(array_length(graph_ref_targets(g), 1), 1,
		'one import statement produces one ref') AS r6,
	ok('esac://app/`./util`'::moniker = ANY(graph_ref_targets(g)),
		'ref target moniker matches the imported path exactly') AS r7
FROM g;

SELECT * FROM finish();

ROLLBACK;
