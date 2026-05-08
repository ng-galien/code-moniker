# pg_code_moniker

[![CI](https://github.com/ng-galien/pg_code_moniker/actions/workflows/ci.yml/badge.svg)](https://github.com/ng-galien/pg_code_moniker/actions/workflows/ci.yml)
[![License: MIT or Apache 2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)](https://www.rust-lang.org)
[![pgrx](https://img.shields.io/badge/pgrx-0.18-darkgreen)](https://github.com/pgcentralfoundation/pgrx)
[![PostgreSQL](https://img.shields.io/badge/postgresql-17-336791)](https://www.postgresql.org)

PostgreSQL extension providing a native type for code symbol identity (`moniker`) and code graph storage (`code_graph`), with an indexed algebra for symbol-level queries.

Implementation: **Rust** via [`pgrx`](https://github.com/pgcentralfoundation/pgrx) 0.18.

## TL;DR

```sql
CREATE EXTENSION pg_code_moniker;

-- Extract a TypeScript file into a code_graph value.
SELECT extract_typescript(
  'src/util.ts',
  'export class Util { run() { return 1; } }',
  'pcm+moniker://app'::moniker
);
-- => code_graph(defs=3, refs=0)

-- Identity is a first-class type: parse, compare, index, JOIN on it.
SELECT 'pcm+moniker://app/lang:ts/dir:src/module:util/class:Util'::moniker
    <@ 'pcm+moniker://app/lang:ts'::moniker;
-- => true (subtree containment, GiST-indexed)
```

Five extractors (TypeScript, Rust, Java, Python, PL/pgSQL) emit defs and refs with full metadata (visibility, signature, binding, …). Cross-file linkage is a single indexed JOIN on `bind_match`. The extension owns no tables — types, operators, and pure functions only.

→ [Posture](#posture) · [Quickstart with Docker](#quickstart-with-docker) · [Building from source](#building-and-testing) · [`SPEC.md`](SPEC.md)

## Posture

A program is a strict canonical tree. Each node has a unique identity called a **moniker**. The moniker is a native PostgreSQL type with operators (`=`, `bind_match`, `<@`, `@>`, `||`, `~`) and a custom GiST index. Matching a reference to its definition is a JOIN on `bind_match` (cross-file) or `=` (total identity) — there is no separate linker phase.

A program element is stored as a **module**: a `code_graph` value (compact varlena) carrying the intra-module structure (defs, refs, containment tree), optionally co-located with source text.

The extension is **stateless**. It owns no tables, exposes only types, operators, and pure functions. Storage and querying are PostgreSQL's job.

## Status

Phases 1–6 of `SPEC.md` shipped: typed canonical URI (`<scheme>+moniker://...`), `moniker` and `code_graph` SQL types with custom Datum layout, btree / hash / GiST opclasses, GIN over `moniker[]`, compact projection (`moniker_compact` / `match_compact`), and six extractors:

| extractor             | grammar           | manifest parser            |
|-----------------------|-------------------|----------------------------|
| `extract_typescript`  | tree-sitter (TS/TSX/JS/JSX) | `extract_package_json` |
| `extract_rust`        | tree-sitter       | `extract_cargo`            |
| `extract_java`        | tree-sitter       | `extract_pom_xml`          |
| `extract_python`      | tree-sitter       | `extract_pyproject`        |
| `extract_go`          | tree-sitter       | `extract_go_mod`           |
| `extract_plpgsql`     | libpg_query (vendored) | —                     |

Each emits defs and refs with full metadata (visibility, signature, alias, confidence, receiver_hint, scope-tracked locals). All take a `deep := false` default; pass `deep := true` for parameter / local extraction.

Phase 7 in flight: `bind_match` operator, the `lang:` segment, and the `binding` column on def / ref records — the three coordinated changes that unlock cross-file linkage.

A multi-project dogfood panel (`test/dogfood/`) validates extractor coverage at scale across zod, date-fns, gson, httpx, pgTAP, clap, bytes, gorilla/mux, and this repo itself.

## Scope

- Types: `moniker`, `moniker_pattern`, `code_graph`.
- Algebra: equality, structural matching, containment, composition, pattern matching.
- Indexes: btree / hash / GiST on `moniker`, GIN over `moniker[]`.
- Per-language extractors producing `code_graph` from source.
- Constructors for synthetic `code_graph` (forward modeling, declared externals).

The first consumer is ESAC. Extraction parity targets vs ESAC's existing extractors are in [`docs/EXTRACTION_TARGETS.md`](docs/EXTRACTION_TARGETS.md). The URI design and segment semantics are in [`docs/MONIKER_URI.md`](docs/MONIKER_URI.md).

## Non-scope

- No table schemas, no triggers, no application logic.
- No project-level configuration storage (callers pass anchors and presets as arguments).
- No cross-project federation.
- No stack-graph-style dynamic resolution; relies on locally determinable monikers.

## Layout

```
src/
  core/     pure Rust, no pgrx, testable with `cargo test`
            moniker/, uri/, code_graph.rs
  pg/      pgrx wrappers exposing the SQL surface (gated by pgN feature)
            moniker/, code_graph/, extract.rs, build.rs
  lang/    per-language extractors (tree-sitter + libpg_query for SQL)
            ts/, rs/, java/, python/, go/, sql/
test/
  sql/                pgTAP files (run via ./test/run.sh)
  dogfood.sh          multi-project ingestion runner
  dogfood/panel.sh    pinned panel of representative open-source projects
examples/
  bench_codegraph.rs  CodeGraph add_def / add_ref scaling bench
  bench_extract.rs    full extractor on a real file
vendor/plpgsql/       vendored PG PL/pgSQL parser sources for libpg_query-style use
Dockerfile            multi-stage build, lands the extension on postgres:17
```

## Quickstart with Docker

The repo ships a multi-stage `Dockerfile` that builds the extension against an apt PostgreSQL 17 install and lands the artifacts on top of the official `postgres:17` image. No local Rust or pgrx setup required:

```sh
docker build -t pg_code_moniker:dev .
docker run --rm -e POSTGRES_PASSWORD=pgcm -p 5432:5432 \
    --name pgcm pg_code_moniker:dev
```

In another shell:

```sh
docker exec -it pgcm psql -U postgres -c "CREATE EXTENSION pg_code_moniker;"
docker exec -it pgcm psql -U postgres -c "
    SELECT extract_typescript(
        'src/util.ts',
        'export class Util { run() { return 1; } }',
        'pcm+moniker://app'::moniker
    );"
```

The build is reproducible and version-pinned (`PG_MAJOR=17`, `PGRX_VERSION=0.18.0`); override either via `--build-arg` if you need a different combination.

## Building and testing

Pure-Rust core and extractors (no PG required):

```sh
cargo test --features pg17 --no-default-features --lib
cargo build --features pg17 --no-default-features
```

Full extension (requires [cargo-pgrx](https://github.com/pgcentralfoundation/pgrx) and an initialized PG toolchain):

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

Then run the suite (drops/recreates a test DB, runs every `test/sql/*.sql`):

```sh
cargo pgrx start pg17
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
./test/run.sh
```

Validate extractors at corpus scale:

```sh
./test/dogfood.sh                     # full panel
./test/dogfood.sh --only zod          # one project
./test/dogfood.sh --reset             # discard caches and re-clone
```

Benchmarks:

```sh
cargo run --release --features pg17 --no-default-features --example bench_codegraph
cargo run --release --features pg17 --no-default-features --example bench_extract
```

## Development workflow

A pre-commit hook ships under `.githooks/`. Activate once per clone:

```sh
git config core.hooksPath .githooks
```

The hook runs `cargo fmt --all -- --check` then `cargo clippy --features pg17 --no-default-features --tests --no-deps -- -D warnings` whenever staged changes touch `*.rs` or `Cargo.{toml,lock}`. It is a no-op for documentation-only commits.

Project formatting convention (`rustfmt.toml`): `hard_tabs = true`.

Canonical loop after a non-trivial change:

```sh
cargo check --features pg17 --no-default-features --tests   # FFI/lifetime check, seconds
cargo test  --features pg17 --no-default-features --lib     # unit tests, sub-second
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
./test/run.sh                                               # pgTAP suite, ~5s
./test/dogfood.sh --only <project>                          # scaling validation
```

## Canonical usage

The extension defines no tables. The shape below — one row per module — is what `SPEC.md` is designed to serve and what ESAC uses.

```sql
CREATE EXTENSION pg_code_moniker;

CREATE TABLE module (
    id          uuid PRIMARY KEY,
    graph       code_graph NOT NULL,
    source_text text,
    source_uri  text,
    origin      text NOT NULL  -- 'extracted' | 'symbolic' | 'external'
);

-- Module identity is its root moniker. Make it queryable + unique without
-- introducing a redundant column.
CREATE UNIQUE INDEX module_root_uniq
    ON module ((graph_root(graph)));

CREATE INDEX module_root_gist
    ON module USING gist ((graph_root(graph)));

-- Cross-module navigation.
CREATE INDEX module_def_monikers_gin
    ON module USING gin (graph_def_monikers(graph));
CREATE INDEX module_ref_targets_gin
    ON module USING gin (graph_ref_targets(graph));

-- Populate from a TS source.
INSERT INTO module (id, graph, source_text, source_uri, origin) VALUES
    (gen_random_uuid(),
     extract_typescript(
         'src/util.ts',
         'export class Util { run() { return 1; } }',
         'pcm+moniker://app'::moniker
     ),
     'export class Util { run() { return 1; } }',
     'src/util.ts',
     'extracted');

-- Find the module that defines a moniker (uses module_def_monikers_gin).
SELECT id FROM module
 WHERE graph_def_monikers(graph)
       @> ARRAY['pcm+moniker://app/lang:ts/dir:src/module:util/class:Util'::moniker];

-- Inspect every def of a module (kind, visibility, signature, binding, …).
SELECT * FROM module m, graph_defs(m.graph) WHERE m.id = $1;

-- Subtree containment: every module under a srcset.
SELECT id FROM module
 WHERE graph_root(graph) <@ 'pcm+moniker://app/srcset:main'::moniker;
```

## Consumers

Any system that needs symbol-level indexing of code (IDE tooling, agentic coding workflows, code search, dead-code analysis). The first consumer in development is ESAC.

## License

Dual-licensed under either of:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option. This matches the convention of the surrounding ecosystem (pgrx, tokio, serde).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project shall be dual-licensed as above, without any additional terms or conditions.
