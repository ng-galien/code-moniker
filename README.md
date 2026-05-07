# pg_code_moniker

PostgreSQL extension providing a native type for code symbol identity (`moniker`) and code graph storage (`code_graph`), with an indexed algebra for symbol-level queries.

Implementation: **Rust** via [`pgrx`](https://github.com/pgcentralfoundation/pgrx).

## Posture

A program is a strict canonical tree. Each node has a unique identity called a **moniker**. The moniker is a native PostgreSQL type with operators (`=`, `<@`, `@>`, `||`, `~`) and a custom GiST index. The matching of a reference to its definition is a JOIN on `=` — there is no separate linker phase.

A program element is stored as a **module** : a `code_graph` value (compact bytea) carrying the internal structure (defs, refs, intra-module tree), optionally co-located with source text.

The extension is **stateless**. It reads no tables, exposes only types, operators, and pure functions. Storage and querying are PostgreSQL's job.

## Status

Pre-implementation. Spec in `SPEC.md`. Phase 1 (`moniker` minimal + URI I/O + `=`) in progress.

## Scope

- Types : `moniker`, `moniker_pattern`, `code_graph`.
- Algebra : equality, containment, composition, pattern matching.
- Index : custom GiST opclass on `moniker`.
- Per-language extractors (tree-sitter based) producing `code_graph` from source.
- Constructors for synthetic `code_graph` (forward modeling, external dependency declarations).

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
cargo pgrx init                       # downloads and builds the PG versions you need
cargo pgrx run pg17                   # builds and loads the extension into a dev PG
cargo pgrx test pg17                  # runs in-PG integration tests
```

## Consumers

Any system that needs symbol-level indexing of code (IDE tooling, agentic coding workflows, code search, dead-code analysis). The first consumer in development is ESAC.

## License

TBD.
