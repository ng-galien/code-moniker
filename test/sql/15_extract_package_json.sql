-- TS build manifest extraction: extract_package_json.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(8);

SELECT has_function('extract_package_json'::name, ARRAY['text'],
	'extract_package_json(text) is exposed');

WITH parsed AS (
	SELECT * FROM extract_package_json($t$
{
	"name": "demo",
	"version": "0.1.0",
	"dependencies": {
		"react": "^18.0.0",
		"@scope/pkg": "1.0.0"
	},
	"devDependencies": {
		"vitest": "1.0.0"
	},
	"peerDependencies": {
		"react-dom": "^18.0.0"
	},
	"optionalDependencies": {
		"fsevents": "2.0.0"
	}
}
$t$)
)
SELECT
	is((SELECT version FROM parsed WHERE name = 'demo' AND dep_kind = 'package'),
		'0.1.0',
		'package row carries name + version + dep_kind=package') AS r1,
	is((SELECT version FROM parsed WHERE name = 'react'),
		'^18.0.0',
		'string-form dep keeps version') AS r2,
	is((SELECT import_root FROM parsed WHERE name = '@scope/pkg'),
		'@scope/pkg',
		'scoped package keeps full @scope/name as import_root') AS r3,
	is((SELECT dep_kind FROM parsed WHERE name = 'vitest'),
		'dev',
		'devDependencies tagged dep_kind=dev') AS r4,
	is((SELECT dep_kind FROM parsed WHERE name = 'react-dom'),
		'peer',
		'peerDependencies tagged dep_kind=peer') AS r5,
	is((SELECT dep_kind FROM parsed WHERE name = 'fsevents'),
		'optional',
		'optionalDependencies tagged dep_kind=optional') AS r6;

-- Linkage demo: a refs subset matched to a pkg table populated from
-- package.json.
CREATE TEMP TABLE pkg(project moniker, name text, version text);
INSERT INTO pkg
	SELECT 'esac+moniker://app'::moniker, name, version
	FROM extract_package_json($t$
{ "name": "demo", "version": "0.1.0",
  "dependencies": { "react": "^18.0.0", "lodash": "^4.0.0" } }
$t$);

WITH g AS (
	SELECT extract_typescript(
		'src/index.ts',
		'import React from "react";
import { get } from "lodash";
import { local } from "./util";',
		'esac+moniker://app'::moniker
	) AS g
), refs_with_root AS (
	SELECT external_pkg_root(t) AS root
	FROM g, LATERAL unnest(graph_ref_targets(g)) t
)
SELECT
	is((SELECT count(*)::int FROM refs_with_root r JOIN pkg p ON p.name = r.root),
		2,
		'JOIN matches refs to packages declared in package.json (react, lodash)') AS r7;

SELECT * FROM finish();

ROLLBACK;
