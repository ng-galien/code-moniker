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

Phases 1–6 of SPEC shipped. v2 chantier ship: typed canonical URI (`<scheme>+moniker://<project>/<kind>:<name>...`), kind names embedded in bytes (no backend-local registry), seg_count dropped → byte-lex order strictly tree-friendly (`m >= ancestor AND m < ancestor||sentinel` works on plain btree). Custom Datum + GiST opclass shipped. Compact projection (`moniker_compact` / `match_compact`) ships as a one-way display. Next backlog: per-language extractors beyond TS (Java, Python, Rust); benchmarks at corpus 10⁴/10⁶. Detail in TODO.md (gitignored).

## Comment sobriety

- **Code**: minimal comments. No narrating what the code does — names and flow already tell that. No essays in module `//!` docstrings: a short paragraph suffices.
- **Tests**: this is the legitimate place for documentation. A short description of the invariant tested is welcome. The test name is the spec (`extract_simple_class_emits_class_def`).
- No emoji. No "smart" framing. Sober, technical.

## Layout

```
src/
  lib.rs              entry, gates pgrx behind pgN features
  core/               pure Rust, no pgrx, testable with cargo test
    moniker/          owned moniker type + tree-position queries
      mod.rs          Moniker struct, Ord (byte-lex), re-exports
      encoding.rs     v2 byte layout (no seg_count), EncodingError, LE helpers
      view.rs         MonikerView + SegmentIter + is_ancestor_of (byte-prefix fast path)
      builder.rs      MonikerBuilder + from_view + truncate
      query.rs        parent / last_kind on Moniker
    uri/              typed canonical URI parse / serialize, backtick escaping
      mod.rs          UriError, UriConfig, re-exports
      parse.rs        from_uri (`<scheme>+moniker://<project>/<kind>:<name>...`)
      serialize.rs    to_uri + name escape
    code_graph.rs     defs / refs / tree per module (kinds as byte strings)
  pg/                 pgrx wrappers, gated behind pgN feature
    mod.rs            module declarations + pcm_version smoke
    registry.rs       DEFAULT_CONFIG (canonical scheme constant)
    moniker/          moniker SQL type + operators
      mod.rs          PostgresType + InOutFuncs + manual varlena Datum + `=`
      query.rs        <@ / @> / parent_of / kind_of / path_of / compose_child
      index.rs        btree + hash opclasses
      gist.rs         GiST opclass (`=`, `@>`, `<@`); page sigs share v2 header
      compact.rs      moniker_compact + match_compact (display projection)
    code_graph.rs     code_graph SQL type + constructors + accessors
    extract.rs        extract_typescript SQL entry point
  lang/               per-language extractors
    mod.rs
    ts/               TS extractor (the canonical layout for new langs)
      mod.rs          pub fn parse, pub fn extract
      walker.rs       AST traversal + def emitters (class, method, function)
      canonicalize.rs moniker construction (compute_module_moniker, extend_*)
      refs.rs         refs extraction (imports today; calls/extends to come)
      kinds.rs        TS kind name byte constants (PATH, CLASS, METHOD, …)
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
- **Iteration loop** : `cargo check --features pg17 --no-default-features` before `cargo pgrx install` — surfaces FFI errors in seconds; install is the slow last mile. The pgTAP runner recreates the DB but does NOT reinstall the extension; install first.
- **Visibility for cross-layer constants** : `core/` items consumed by `pg/` need `pub(crate)`, not `pub(super)`. The `core::moniker::encoding` constants (`VERSION`, `HEADER_FIXED_LEN`, `read_u16`, `write_u16`) are the canonical example.

## pgrx 0.18 manual Datum

`moniker` ships its bytes as a raw varlena, not the default cbor wrapper. To keep `#[derive(PostgresType)]` for the SQL DDL emission while replacing the cbor `IntoDatum`/`FromDatum`, use the opt-out attribute `#[bikeshed_postgres_type_manually_impl_from_into_datum]` and provide the five impls manually (`IntoDatum`, `FromDatum`, `BoxRet`, `UnboxDatum`, `ArgAbi`). The macro source at `pgrx-macros-0.18.0/src/lib.rs:902-973` is the canonical reference for the shape — mirror it, swap cbor encode/decode for varlena helpers (`pgrx::set_varsize_4b`, `pgrx::varlena_to_byte_slice`, `pg_sys::pg_detoast_datum_packed`).

**GIN bulk-build trap.** `rust_regtypein("X")` raises `type "X" does not exist` under restricted search_path (`CREATE INDEX USING gin (fn(graph))` over existing rows). Cache the OID in `OnceLock`, look up via `get_extension_oid` → `get_extension_schema` → `get_namespace_name` → `regtypein("schema.X")`. See `moniker_type_oid` in `src/pg/moniker/mod.rs`.

**Adding a `#[pg_extern]` arg without breaking callers**: wrap the new param in `pgrx::default!(T, "sql_literal")`. Existing SQL callsites stay valid; opt in via named arg (`fn extract_rust(... , deep := true)`).

## tree-sitter-rust gotchas

- Node kinds are `function_item` / `type_item` / `enum_item` / `trait_item` (not `fn_item` / `type_alias_item`).
- Closure `parameters` field is `closure_parameters`; children are bare patterns (`|x|`) OR `parameter` wrappers (`|x: i32|`). Counting only `kind == "parameter"` undercounts untyped closures.
- Statement-position `if_expression` / `match_expression` is wrapped in `expression_statement`. A body-walker dropping that kind loses locals nested in `if cond { let x = … }`.
