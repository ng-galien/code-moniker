
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

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
			'code+moniker://app'::moniker)),
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
			'code+moniker://app'::moniker)),
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
			'code+moniker://app'::moniker));


SELECT is(
	(SELECT graph_root(graph)::text FROM module WHERE id = 'repository'),
	'code+moniker://app/lang:ts/dir:src/module:repository',
	'repository module root');

SELECT is(
	(SELECT graph_root(graph)::text FROM module WHERE id = 'service'),
	'code+moniker://app/lang:ts/dir:src/module:service',
	'service module root');


SELECT cmp_ok(
	(SELECT array_length(graph_def_monikers(graph), 1) FROM module WHERE id = 'service'),
	'>=', 5,
	'service graph has at least the module + 2 defs + members');

SELECT ok(
	(SELECT graph @> 'code+moniker://app/lang:ts/dir:src/module:service/class:UserService'::moniker
	   FROM module WHERE id = 'service'),
	'service graph contains UserService class');

SELECT ok(
	(SELECT graph @> 'code+moniker://app/lang:ts/dir:src/module:service/class:UserService/method:findById(id:string)'::moniker
	   FROM module WHERE id = 'service'),
	'service graph contains UserService#findById(id:string) method');


SELECT ok(
	EXISTS (SELECT 1 FROM module
	         WHERE id = 'service'
	           AND 'code+moniker://app/lang:ts/dir:src/module:repository/path:UserRepository'::moniker = ANY(graph_ref_targets(graph))),
	'service ref-targets contains UserRepository under the repository module');

SELECT ok(
	EXISTS (SELECT 1 FROM module
	         WHERE id = 'service'
	           AND 'code+moniker://app/lang:ts/dir:src/module:logger/path:ConsoleLogger'::moniker = ANY(graph_ref_targets(graph))),
	'service ref-targets contains ConsoleLogger under the logger module');


SELECT is(
	(SELECT id FROM module
	  WHERE graph @> 'code+moniker://app/lang:ts/dir:src/module:repository/class:UserRepository'::moniker),
	'repository',
	'graph @> resolves UserRepository to its owning module');

SELECT is(
	(SELECT id FROM module
	  WHERE graph @> 'code+moniker://app/lang:ts/dir:src/module:logger/class:ConsoleLogger'::moniker),
	'logger',
	'graph @> resolves ConsoleLogger to its owning module');



SELECT is(
	(SELECT array_agg(id ORDER BY id) FROM module
	  WHERE EXISTS (
	    SELECT 1 FROM graph_refs(graph) r
	    WHERE r.kind IN ('imports_symbol','imports_module','reexports')
	      AND 'code+moniker://app/lang:ts/dir:src/module:repository'::moniker @> r.target
	  )),
	ARRAY['service']::text[],
	'ancestor query on import refs finds every importer of the repository module');

SELECT * FROM finish();

ROLLBACK;
