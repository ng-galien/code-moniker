
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(12);


SELECT has_function('extract_cargo'::name, ARRAY['text'],
	'extract_cargo(text) is exposed');

SELECT has_function('external_pkg_root'::name, ARRAY['moniker'],
	'external_pkg_root(moniker) is exposed');


WITH parsed AS (
	SELECT * FROM extract_cargo($t$
[package]
name = "demo"
version = "0.1.0"

[dependencies]
serde = "1.0"
tokio = { version = "1.40", features = ["full"] }
local_lib = { path = "../local_lib" }
tree-sitter = "0.26"

[dev-dependencies]
criterion = "0.5"
$t$)
)
SELECT
	is((SELECT version FROM parsed WHERE name = 'demo' AND dep_kind = 'package'),
		'0.1.0',
		'package row carries name + version + dep_kind=package') AS r1,
	is((SELECT version FROM parsed WHERE name = 'serde'),
		'1.0',
		'string-form dep keeps version') AS r2,
	is((SELECT version FROM parsed WHERE name = 'tokio'),
		'1.40',
		'table-form dep extracts version field') AS r3,
	is((SELECT version FROM parsed WHERE name = 'local_lib'),
		NULL,
		'path-only dep has NULL version') AS r4,
	is((SELECT dep_kind FROM parsed WHERE name = 'criterion'),
		'dev',
		'dev-dependencies tagged dep_kind=dev') AS r5,
	is((SELECT import_root FROM parsed WHERE name = 'tree-sitter'),
		'tree_sitter',
		'hyphenated cargo name normalized to underscore import_root') AS r5b;


SELECT is(
	external_pkg_root('esac+moniker://app/external_pkg:pgrx/path:prelude'::moniker),
	'pgrx',
	'first external_pkg segment surfaces as the root');

SELECT ok(
	external_pkg_root('esac+moniker://app/path:src/path:lib'::moniker) IS NULL,
	'project-local moniker returns NULL');

CREATE TEMP TABLE pkg(project moniker, name text, version text);
INSERT INTO pkg VALUES
	('esac+moniker://app'::moniker, 'pgrx', '0.18'),
	('esac+moniker://app'::moniker, 'serde', '1.0');

WITH g AS (
	SELECT extract_rust(
		'src/lib.rs',
		'use pgrx::prelude::*;
use serde::Serialize;
use std::collections::HashMap;',
		'esac+moniker://app'::moniker
	) AS g
), refs_with_root AS (
	SELECT external_pkg_root(t) AS root
	FROM g, LATERAL unnest(graph_ref_targets(g)) t
)
SELECT
	is((SELECT count(*)::int FROM refs_with_root r JOIN pkg p ON p.name = r.root),
		2,
		'JOIN matches refs to packages declared in Cargo.toml (pgrx, serde)') AS r6,
	is((SELECT count(*)::int FROM refs_with_root r WHERE r.root = 'std'),
		1,
		'unmatched external root (std, not in pkg table) still extractable') AS r7;

SELECT * FROM finish();

ROLLBACK;
