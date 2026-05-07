# CLAUDE.md

`pg_code_moniker` — PostgreSQL extension in Rust + pgrx. Native `moniker` and `code_graph` types with GiST-indexed algebra. No tables, no triggers, no persistent state — **types + operators + per-language extractors**.

First consumer: ESAC. **The compass for every decision: improve ESAC's symbolic experience** (`esac_symbol` find/refs/carriers/families/health/gaps, `esac_outline`), never degrade it. Every line added must be traceable to one of these operations. If a feature does not serve one of these actions, it does not belong in the extension.

## Documents

- `README.md` — posture, scope, build/test commands
- `SPEC.md` — conceptual model (canonical tree, moniker, code_graph, srcset, three origins), public API, SCIP URI format, implementation phases
- `CLAUDE.md` (this file) — coding rules and progress state

No work-in-progress archives, no decision memos, no speculative docs. Git log + the code + these three files are the source of truth.

## Comment sobriety

- **Code**: minimal comments. No narrating what the code does — names and flow already tell that. No essays in module `//!` docstrings: a short paragraph suffices.
- **Tests**: this is the legitimate place for documentation. A short description of the invariant tested is welcome. The test name is the spec (`extract_simple_class_emits_class_def`).
- No emoji. No "smart" framing. Sober, technical.

## Layout

```
src/
  lib.rs              entry, gates pgrx behind pgN features
  core/               pure Rust, no pgrx, testable with cargo test
    kind_registry.rs  KindId + PunctClass (Path/Type/Term/Method)
    moniker.rs        bytea encoding + builder + view + iterator
    uri/              SCIP parse / serialize, backtick escaping
      mod.rs          UriError, UriConfig, re-exports
      parse.rs        from_uri + read_name + read_arity
      serialize.rs    to_uri + escape helpers
    code_graph.rs     defs / refs / tree per module
  pg/                 pgrx wrappers, gated behind pgN feature
  lang/               per-language extractors
    mod.rs
    ts/               target: one sub-directory per language
      mod.rs          pub fn parse, pub fn extract
      walker.rs       AST traversal
      canonicalize.rs moniker construction from AST nodes
      refs.rs         refs extraction (imports, calls, extends, ...)
      kinds.rs        language-specific kind interning
    java/             future
    python/           future
    pgsql/            future
test/sql/             pgTAP test files (run via ./test/run.sh)
tests/fixtures/<lang>/   source fixtures with expected code_graph snapshots
```

`lang/ts.rs` is currently monolithic (~280 lines); split it per the target layout once it exceeds ~400 lines. **No file > ~600 lines.** One responsibility per file, named by its suffix.

## TDD

Tests describe the contract before the implementation. Cycle: red test → minimal impl → green → next cycle. Tests inline in `#[cfg(test)] mod tests` next to the code under test — standard Rust convention, access to private items without ceremony. When a file exceeds the cap, split the production module (subfiles with their own `mod tests`); do not extract the tests. `cargo test` for `core/` and `lang/` (pure Rust, no PG). For the SQL surface exposed by `pg/` we use **pgTAP**: tests in `test/sql/*.sql`, runner `./test/run.sh` which leans on the PG17 instance managed by pgrx. No `pgrx-tests` / `#[pg_test]` — SQL is tested in SQL.
