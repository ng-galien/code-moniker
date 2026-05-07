# pg_code_moniker

PostgreSQL extension providing a native type for code symbol identity (`moniker`) and code graph storage (`code_graph`), with an indexed algebra for symbol-level queries.

Implementation: **Rust** via [`pgrx`](https://github.com/pgcentralfoundation/pgrx).

## Posture

A program is a strict canonical tree. Each node has a unique identity called a **moniker**. The moniker is a native PostgreSQL type with operators (`=`, `<@`, `@>`, `||`, `~`) and a custom GiST index. The matching of a reference to its definition is a JOIN on `=` — there is no separate linker phase.

A program element is stored as a **module** : a `code_graph` value (compact bytea) carrying the internal structure (defs, refs, intra-module tree), optionally co-located with source text.

The extension is **stateless**. It reads no tables, exposes only types, operators, and pure functions. Storage and querying are PostgreSQL's job.

## Status

Phases 1–5 of the SPEC complete. Phase 6 (custom GiST opclass + benchmarks) in progress: btree and hash opclasses on `moniker` shipped — `ORDER BY`, `DISTINCT`, hash-join, and `GIN` on `moniker[]` all work. The custom GiST opclass for tree-containment queries (`<@` / `@>` indexed) is the remaining piece. Roughly 84 pure-Rust + 70 pgTAP tests cover the surface end to end.

## Scope

- Types : `moniker`, `moniker_pattern`, `code_graph`.
- Algebra : equality, containment, composition, pattern matching.
- Index : custom GiST opclass on `moniker`.
- Per-language extractors (tree-sitter based) producing `code_graph` from source.
- Constructors for synthetic `code_graph` (forward modeling, external dependency declarations).

Extraction targets for ESAC are documented in
[`docs/EXTRACTION_TARGETS.md`](docs/EXTRACTION_TARGETS.md). The extension must
reach parity with ESAC's existing extractors before replacing them.

## Non-scope

- No table schemas, no triggers, no application logic.
- No project-level configuration (callers pass it in as arguments).
- No cross-project federation.
- No stack-graph-style dynamic resolution (relies on locally determinable monikers).

## Layout

```
src/
  core/     Pure Rust, no pgrx. The type internals (kind registry,
            moniker encoding, code_graph layout, operators). Testable
            with `cargo test` -- no PG required.
  pg/       pgrx wrappers exposing the SQL surface. Built only with a
            `pgN` feature.
  lang/     Per-language extractors (tree-sitter). Pure Rust.
```

## Building & testing

Pure-Rust core (no PG required):

```sh
cargo test            # runs all unit tests in src/core/
cargo build           # builds the core; pg/ is feature-gated and skipped
```

Full extension (requires [cargo-pgrx](https://github.com/pgcentralfoundation/pgrx) and an initialised PG toolchain):

```sh
cargo install --locked cargo-pgrx
cargo pgrx init --pg17 download       # downloads and builds PG17 (one-time, ~15 min)
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
cargo pgrx run pg17                   # interactive psql shell with the extension loaded
```

SQL-side tests use [pgTAP](https://pgtap.org/). Install once against the pgrx PG17:

```sh
git clone --depth 1 https://github.com/theory/pgtap.git /tmp/pgtap-build
PG_CONFIG=$HOME/.pgrx/17.9/pgrx-install/bin/pg_config make -C /tmp/pgtap-build install
```

Then run the suite (starts pgrx PG17 if needed, drops/recreates a test DB, runs `test/sql/*.sql`):

```sh
cargo pgrx start pg17
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
./test/run.sh
```

## Canonical usage

The extension defines no tables. The pattern below — one row per module — is the canonical shape SPEC is designed to serve, and the one ESAC uses.

```sql
CREATE EXTENSION pg_code_moniker;

CREATE TABLE module (
    id          uuid PRIMARY KEY,
    graph       code_graph NOT NULL,
    source_text text,
    source_uri  text,
    origin      text NOT NULL  -- 'extracted' | 'symbolic' | 'external'
);

-- Module identity = its root moniker. Make it queryable + unique without
-- introducing a redundant column.
CREATE UNIQUE INDEX module_root_uniq
    ON module ((graph_root(graph)));

-- Cross-module navigation indexes. The btree + hash opclasses on moniker
-- (shipped in Phase 6) make `array_ops` work on moniker[], so these GIN
-- indexes resolve the SPEC linkage pattern in O(log n).
CREATE INDEX module_def_monikers_gin
    ON module USING gin (graph_def_monikers(graph));
CREATE INDEX module_ref_targets_gin
    ON module USING gin (graph_ref_targets(graph));

-- Populate from a TS source.
INSERT INTO module (id, graph, source_text, source_uri, origin) VALUES
    (gen_random_uuid(),
     extract_typescript('src/util.ts',
         'export class Util { run() { return 1; } }',
         'esac://app'::moniker),
     'export class Util { run() { return 1; } }',
     'src/util.ts',
     'extracted');

-- Find the module that defines a moniker (uses module_def_monikers_gin).
SELECT id FROM module
 WHERE graph_def_monikers(graph) @> ARRAY['esac://app/src/util#Util#'::moniker];

-- Find every module that references a moniker (uses module_ref_targets_gin).
SELECT id FROM module
 WHERE graph_ref_targets(graph) @> ARRAY['esac://app/src/util'::moniker];
```

Subtree containment queries (`graph_root <@ 'esac://app/main'::moniker`) need a custom GiST opclass on `moniker`, scheduled but not yet shipped.

## Consumers

Any system that needs symbol-level indexing of code (IDE tooling, agentic coding workflows, code search, dead-code analysis). The first consumer in development is ESAC.

## License

TBD.
