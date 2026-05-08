
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(4);

CREATE TEMP TABLE module (
	id    text       PRIMARY KEY,
	graph code_graph NOT NULL
);
CREATE INDEX module_def_monikers_gin
	ON module USING gin (graph_def_monikers(graph));
CREATE INDEX module_ref_targets_gin
	ON module USING gin (graph_ref_targets(graph));

INSERT INTO module (id, graph) VALUES
	('lib', extract_typescript('src/lib.ts',
		'export class Lib { go() { return 1; } }',
		'esac+moniker://app'::moniker)),
	('app', extract_typescript('src/app.ts',
		'import { Lib } from "./lib";',
		'esac+moniker://app'::moniker));

SET LOCAL enable_seqscan = off;

CREATE OR REPLACE FUNCTION plan_uses(qry text, fragment text) RETURNS bool
	LANGUAGE plpgsql AS $$
DECLARE
	line text;
BEGIN
	FOR line IN EXECUTE 'EXPLAIN ' || qry LOOP
		IF strpos(line, fragment) > 0 THEN
			RETURN true;
		END IF;
	END LOOP;
	RETURN false;
END $$;


SELECT ok(
	plan_uses(
		$$SELECT id FROM module WHERE graph_def_monikers(graph) @> ARRAY['esac+moniker://app/lang:ts/path:src/path:lib/class:Lib'::moniker]$$,
		'module_def_monikers_gin'),
	'graph_def_monikers @> ARRAY[m] uses module_def_monikers_gin');

SELECT ok(
	plan_uses(
		$$SELECT id FROM module WHERE graph_def_monikers(graph) @> ARRAY['esac+moniker://app/lang:ts/path:src/path:lib/class:Lib'::moniker]$$,
		'Bitmap Index Scan'),
	'planner emits a Bitmap Index Scan node for the def lookup');


SELECT ok(
	plan_uses(
		$$SELECT id FROM module WHERE graph_ref_targets(graph) @> ARRAY['esac+moniker://app/lang:ts/path:src/path:lib'::moniker]$$,
		'module_ref_targets_gin'),
	'graph_ref_targets @> ARRAY[m] uses module_ref_targets_gin');

SELECT ok(
	plan_uses(
		$$SELECT id FROM module WHERE graph_ref_targets(graph) @> ARRAY['esac+moniker://app/lang:ts/path:src/path:lib'::moniker]$$,
		'Bitmap Index Scan'),
	'planner emits a Bitmap Index Scan node for the ref lookup');

SELECT * FROM finish();

ROLLBACK;
