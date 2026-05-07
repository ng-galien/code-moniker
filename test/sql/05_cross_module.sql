-- Phase 5: cross-module linkage. Three TS modules, two of them import
-- the third. SQL JOIN on graph_def_monikers / graph_ref_targets
-- resolves links — all of it through the extension's operators alone,
-- no extractor-level resolution.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(10);

CREATE TEMP TABLE module (
	id    text       PRIMARY KEY,
	graph code_graph NOT NULL
);

INSERT INTO module (id, graph) VALUES
	('repository',
		extract_typescript(
			'src/repository.ts',
			$ts$
export class UserRepository {
	findById(id: string) { return null; }
	findAll() { return []; }
	save(user: object) { return user; }
}

export function makeUserRepository() {
	return new UserRepository();
}
$ts$,
			'esac+moniker://app'::moniker)),
	('logger',
		extract_typescript(
			'src/logger.ts',
			$ts$
export class ConsoleLogger {
	debug(msg: string) { return msg; }
	info(msg: string)  { return msg; }
	warn(msg: string)  { return msg; }
}
$ts$,
			'esac+moniker://app'::moniker)),
	('service',
		extract_typescript(
			'src/service.ts',
			$ts$
import { UserRepository, makeUserRepository } from "./repository";
import { ConsoleLogger } from "./logger";

export class UserService {
	constructor() {
		this.repo = makeUserRepository();
		this.log  = new ConsoleLogger();
	}
	findById(id: string) { return this.repo.findById(id); }
	findAll()            { return this.repo.findAll(); }
}

export function bootApp() {
	return new UserService();
}
$ts$,
			'esac+moniker://app'::moniker));

-- Each module's root reflects its on-disk path under the anchor -----------

SELECT is(
	(SELECT graph_root(graph)::text FROM module WHERE id = 'repository'),
	'esac+moniker://app/path:src/path:repository',
	'repository module root');

SELECT is(
	(SELECT graph_root(graph)::text FROM module WHERE id = 'service'),
	'esac+moniker://app/path:src/path:service',
	'service module root');

-- Service exposes its expected defs (root, two top-level + their members) -

SELECT cmp_ok(
	(SELECT array_length(graph_def_monikers(graph), 1) FROM module WHERE id = 'service'),
	'>=', 5,
	'service graph has at least the module + 2 defs + members');

SELECT ok(
	(SELECT graph @> 'esac+moniker://app/path:src/path:service/class:UserService'::moniker
	   FROM module WHERE id = 'service'),
	'service graph contains UserService class');

SELECT ok(
	(SELECT graph @> 'esac+moniker://app/path:src/path:service/class:UserService/method:findById(string)'::moniker
	   FROM module WHERE id = 'service'),
	'service graph contains UserService#findById(string) method');

-- Cross-module link: service.ts imports point at named symbols anchored
-- under the imported module's own moniker (resolved relative to the
-- importer dir, then suffixed with `/path:<name>` per specifier).

SELECT ok(
	EXISTS (SELECT 1 FROM module
	         WHERE id = 'service'
	           AND 'esac+moniker://app/path:src/path:repository/path:UserRepository'::moniker = ANY(graph_ref_targets(graph))),
	'service ref-targets contains UserRepository under the repository module');

SELECT ok(
	EXISTS (SELECT 1 FROM module
	         WHERE id = 'service'
	           AND 'esac+moniker://app/path:src/path:logger/path:ConsoleLogger'::moniker = ANY(graph_ref_targets(graph))),
	'service ref-targets contains ConsoleLogger under the logger module');

-- JOIN on `code_graph @> moniker`: which module defines a given moniker?

SELECT is(
	(SELECT id FROM module
	  WHERE graph @> 'esac+moniker://app/path:src/path:repository/class:UserRepository'::moniker),
	'repository',
	'graph @> resolves UserRepository to its owning module');

SELECT is(
	(SELECT id FROM module
	  WHERE graph @> 'esac+moniker://app/path:src/path:logger/class:ConsoleLogger'::moniker),
	'logger',
	'graph @> resolves ConsoleLogger to its owning module');

-- Reverse direction: which modules import the repository? With per-symbol
-- imports, the importer's targets are anchored *under* the repository
-- module — use a moniker-ancestor predicate to flatten that back to the
-- module level.

-- Filter to import-flavoured ref kinds; otherwise a module's own internal
-- refs (`new UserRepository()` etc.) are also anchored under the module
-- moniker by name-keying convention and would self-match.

SELECT is(
	(SELECT array_agg(id ORDER BY id) FROM module
	  WHERE EXISTS (
	    SELECT 1 FROM graph_refs(graph) r
	    WHERE r.kind IN ('imports_symbol','imports_module','reexports')
	      AND 'esac+moniker://app/path:src/path:repository'::moniker @> r.target
	  )),
	ARRAY['service']::text[],
	'ancestor query on import refs finds every importer of the repository module');

SELECT * FROM finish();

ROLLBACK;
