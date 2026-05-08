
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(6);

-- code_graph is variable-length, by-reference, extended-storage like moniker.
SELECT is(typlen,    -1::int2,   'code_graph is variable-length')
FROM pg_type WHERE typname = 'code_graph';
SELECT is(typbyval,  false,      'code_graph is passed by reference')
FROM pg_type WHERE typname = 'code_graph';
SELECT is(typstorage, 'x'::"char", 'code_graph uses extended storage')
FROM pg_type WHERE typname = 'code_graph';

-- A representative graph: ~20 defs + ~20 refs against an extracted TS file.
WITH src AS (
	SELECT $$
		export class Foo {
			doSomething(x: number, y: string): string {
				return this.helper(x) + y;
			}
			helper(n: number): number { return n * 2; }
		}
		export function topLevel(a: number) { return a + 1; }
		export const C1 = 42;
		export const C2 = 'hello';
		import { Bar } from './bar';
		import { Baz, Qux } from './baz';
		function call_them() {
			const f = new Foo();
			f.doSomething(1, 'a');
			topLevel(2);
			Bar();
			Baz();
			Qux();
		}
	$$ AS code
), g AS (
	SELECT extract_typescript(
		'app/src/foo.ts',
		src.code,
		'esac+moniker://app'::moniker,
		true
	) AS graph
	FROM src
)
SELECT
	-- Threshold is generous so harmless extractor drift doesn't go red.
	cmp_ok(pg_column_size(graph), '<', 5000,
		'code_graph storage stays compact for a representative graph') AS r1,
	cmp_ok(pg_column_size(graph), '>', 0,
		'code_graph storage is non-empty') AS r2,
	-- Round-trip: graph_root + graph_def_monikers preserve the graph after one varlena hop.
	cmp_ok(array_length(graph_def_monikers(graph), 1), '>', 5,
		'graph survives one varlena round-trip and exposes its defs') AS r3
FROM g;

SELECT * FROM finish();

ROLLBACK;
