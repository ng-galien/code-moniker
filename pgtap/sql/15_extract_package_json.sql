
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(10);

SELECT has_function('extract_package_json'::name, ARRAY['moniker', 'text'],
	'extract_package_json(moniker, text) is exposed');

WITH parsed AS (
	SELECT * FROM extract_package_json('code+moniker://app'::moniker, $t$
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
		'optionalDependencies tagged dep_kind=optional') AS r6,
	is((SELECT package_moniker FROM parsed WHERE name = 'react'),
		'code+moniker://app/external_pkg:react'::moniker,
		'package_moniker is anchored on the supplied project and head=import_root') AS r6b;

CREATE TEMP TABLE pkg(package_moniker moniker, name text, version text);
INSERT INTO pkg
	SELECT package_moniker, name, version
	FROM extract_package_json('code+moniker://app'::moniker, $t$
{ "name": "demo", "version": "0.1.0",
  "dependencies": { "react": "^18.0.0", "lodash": "^4.0.0" } }
$t$);

WITH g AS (
	SELECT extract_typescript(
		'src/index.ts',
		'import React from "react";
import { get } from "lodash";
import { local } from "./util";',
		'code+moniker://app'::moniker
	) AS g
), ref_targets AS (
	SELECT t AS target
	FROM g, LATERAL unnest(graph_ref_targets(g)) t
)
SELECT
	is((SELECT count(*)::int
		FROM ref_targets r
		JOIN pkg p ON p.package_moniker @> r.target),
		2,
		'package_moniker @> ref.target binds refs declared in package.json (react, lodash)') AS r7,
	is((SELECT count(DISTINCT p.name)::int
		FROM ref_targets r
		JOIN pkg p ON p.package_moniker @> r.target),
		2,
		'each declared package binds at least one ref') AS r8;

SELECT * FROM finish();

ROLLBACK;
