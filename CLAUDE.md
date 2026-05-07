# CLAUDE.md

`pg_code_moniker` — PostgreSQL extension in Rust + pgrx. Native `moniker` and `code_graph` types with GiST-indexed algebra. No tables, no triggers, no persistent state — **types + operators + per-language extractors**.

First consumer: ESAC. **The compass for every decision: improve ESAC's symbolic experience** (`esac_symbol` find/refs/carriers/families/health/gaps, `esac_outline`), never degrade it. Every line added must be traceable to one of these operations. If a feature does not serve one of these actions, it does not belong in the extension.

## Documents

- `README.md` — posture, scope, build/test commands
- `SPEC.md` — conceptual model (canonical tree, moniker, code_graph, srcset, three origins), public API, SCIP URI format, implementation phases
- `CLAUDE.md` (this file) — coding rules and progress state
- `docs/EXTRACTION_TARGETS.md` — parity targets vs ESAC's existing extractors (the bar each new language has to clear)

No work-in-progress archives, no decision memos, no speculative docs. Git log + the code + these files are the source of truth.

## Direction

Phases 1–5 of SPEC shipped. Phase 6 partial: btree + hash opclasses on `moniker`, GIN on `moniker[]`, README pattern canonique — all ✓. The remaining Phase 6 work (custom GiST opclass, pgrx ANALYZE quirk, storage/decode perf) converges on a single chantier: replace `#[derive(PostgresType)]` cbor-wrapping with a manual `IntoDatum`/`FromDatum` varlena. Attack it in a focused session with fast install/run cycles to iterate on the FFI; detail in TODO.md (gitignored).

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
    moniker/          owned moniker type + tree-position queries
      mod.rs          Moniker struct, Ord (byte-lex), re-exports
      encoding.rs     byte layout, EncodingError, LE helpers
      view.rs         MonikerView + SegmentIter + is_ancestor_of (byte-prefix fast path)
      builder.rs      MonikerBuilder + from_view + truncate
      query.rs        parent / last_kind on Moniker
    uri/              SCIP parse / serialize, backtick escaping
      mod.rs          UriError, UriConfig, re-exports
      parse.rs        from_uri + read_name + read_arity
      serialize.rs    to_uri + escape helpers
    code_graph.rs     defs / refs / tree per module
  pg/                 pgrx wrappers, gated behind pgN feature
    mod.rs            module declarations + pcm_version smoke
    registry.rs       process-wide KindRegistry (Mutex), DEFAULT_CONFIG
    moniker/          moniker SQL type + operators
      mod.rs          PostgresType + InOutFuncs + `=` + project_of/depth
      query.rs        <@ / @> / parent_of / kind_of / path_of / compose_child
      index.rs        btree + hash opclasses
    code_graph.rs     code_graph SQL type + constructors + accessors
    extract.rs        extract_typescript SQL entry point
  lang/               per-language extractors
    mod.rs
    ts/               TS extractor (the canonical layout for new langs)
      mod.rs          pub fn parse, pub fn extract
      walker.rs       AST traversal + def emitters (class, method, function)
      canonicalize.rs moniker construction (compute_module_moniker, extend_*)
      refs.rs         refs extraction (imports today; calls/extends to come)
      kinds.rs        TsKinds: canonical structural kinds + semantic labels
    java/             future
    python/           future
    pgsql/            future
test/sql/             pgTAP test files (run via ./test/run.sh)
tests/fixtures/<lang>/   source fixtures with expected code_graph snapshots
```

**No file > ~600 lines.** One responsibility per file, named by its suffix. When a file exceeds the cap, split the production module (subfiles with their own `mod tests`); do not extract the tests.

## TDD

Tests describe the contract before the implementation. Cycle: red test → minimal impl → green → next cycle.

- **Pure-Rust** : `cargo test` for `core/` and `lang/`. Tests inline in `#[cfg(test)] mod tests` next to the code under test — standard Rust convention, access to private items without ceremony.
- **SQL surface** : `pg/` is tested via **pgTAP**, files in `test/sql/*.sql`, runner `./test/run.sh` against the PG17 instance managed by pgrx. No `pgrx-tests` / `#[pg_test]` — SQL is tested in SQL.
