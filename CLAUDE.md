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

Phases 1–6 of SPEC shipped. v2 milestone ship: typed canonical URI (`<scheme>+moniker://<project>/<kind>:<name>...`), kind names embedded in bytes (no backend-local registry), seg_count dropped → byte-lex order strictly tree-friendly (`m >= ancestor AND m < ancestor||sentinel` works on plain btree). Custom Datum + GiST opclass shipped. Compact projection (`moniker_compact` / `match_compact`) ships as a one-way display. CodeGraph carries a moniker→idx HashMap so `find_def` is O(1) at corpus scale. TS, Rust, Java, Python (tree-sitter) and SQL/PL-pgSQL (vendored libpg_query) extractors shipped with full metadata (visibility / signature / alias / confidence / receiver_hint / same-file resolution / scope-tracked locals). Manifest parsers shipped for the four with build systems (`extract_cargo` / `extract_package_json` / `extract_pom_xml` / `extract_pyproject`). Multi-project dogfood panel under `test/dogfood/` for scaling validation. Current effort: Phase 7 cross-file linkage (`bind_match` + `binding` column + `lang:` segment). Detail in TODO.md (gitignored).

## Comment sobriety

- **Code**: minimal comments. No narrating what the code does — names and flow already tell that. No essays in module `//!` docstrings: a short paragraph suffices.
- **Tests**: this is the legitimate place for documentation. A short description of the invariant tested is welcome. The test name is the spec (`extract_simple_class_emits_class_def`).
- No emoji. No "smart" framing. Sober, technical.

## Layout

```
src/
  lib.rs                entry, gates pgrx behind pgN features
  core/                 pure Rust, no pgrx, testable with cargo test
    moniker/            Moniker struct + Ord (byte-lex) + tree-position queries
                        (mod, encoding, view, builder, query)
    uri/                typed canonical URI parse / serialize (mod, parse, serialize)
    code_graph.rs       defs / refs / O(1) moniker→idx index, DefAttrs / RefAttrs
                        (visibility, signature, alias, confidence, receiver_hint)
  pg/                   pgrx wrappers, gated behind pgN feature
    moniker/            moniker SQL type + operators (btree / hash / GiST opclasses,
                        compact projection)
    code_graph.rs       code_graph SQL type + accessors (graph_defs / graph_refs
                        carrying metadata columns)
    extract.rs          extract_typescript / extract_rust / extract_java SQL entries
    build.rs            extract_cargo / extract_package_json / extract_pom_xml
  lang/                 per-language extractors
    kinds.rs            cross-language vocabulary (VIS_* / CONF_* constants).
                        New extractors `pub use` from here; never redeclare.
    ts/                 TypeScript / TSX / JS / JSX
      mod.rs            pub fn parse, pub fn extract, Presets
      kinds.rs          TS-specific structural kinds + pub use of shared
      canonicalize.rs   moniker construction
      walker.rs         AST dispatch + def emitters
      refs.rs           non-import ref emitters
      imports.rs        imports / reexports + target builders
      scope.rs          local-scope tracking + visibility helpers
      build.rs          package.json parser
    rs/                 Rust (mod / kinds / canonicalize / walker / refs / build)
    java/               Java (mod / kinds / canonicalize / walker / refs / scope /
                        build for pom.xml)
test/
  sql/                  pgTAP test files (run via ./test/run.sh)
  dogfood.sh            multi-project ingestion runner
  dogfood/panel.sh      pinned panel of representative open-source projects
  dogfood/README.md     panel doctrine + spot-check queries
examples/
  bench_codegraph.rs    CodeGraph add_def / add_ref scaling bench
  bench_extract.rs      full extractor on a real file (defaults to zod/types.ts)
```

**No file > ~600 lines.** One responsibility per file, named by its suffix. When a file exceeds the cap, split the production module (subfiles with their own `mod tests`); do not extract the tests.

## Extractor extension protocol

A new language under `src/lang/<lang>/` mirrors the `ts/` skeleton:

- `mod.rs` — `pub fn parse`, `pub fn extract(uri, source, anchor, deep, &Presets) -> CodeGraph`, `pub struct Presets` for caller-supplied hints.
- `kinds.rs` — language-specific structural kinds + `pub(super) use crate::lang::kinds::{VIS_*, CONF_*}` for the shared vocabulary. Never redeclare visibility or confidence values.
- `canonicalize.rs` — `compute_module_moniker`, `extend_segment`, `extend_callable` with arity-based segment names.
- `walker.rs` — Walker struct (source bytes, module, deep, presets, scope state, language-specific tables like `imports` / `type_table`) + AST dispatch + def emitters.
- `refs.rs` — ref emitters per kind. Use `RefAttrs { ..RefAttrs::default() }` shorthand so future fields land without touching every site. Reach for `add_ref_attrs` when emitting confidence / alias / receiver_hint; the bare `add_ref` is for cases where nothing is known.
- `scope.rs` — local-scope stack (`record_local`, `is_local_name`, `name_confidence`) and language-specific visibility helper. Defaults differ per language — Java is `package`, TS is `public`. Push/pop on each callable so `confidence: local` stays accurate.
- Optional `imports.rs` (when imports decompose into many specifiers) and `build.rs` (manifest parser yielding `Vec<Dep>` consumed by `src/pg/build.rs::extract_<system>`).

Wire the SQL surface in `src/pg/extract.rs` (`#[pg_extern] fn extract_<lang>(...)`); add a pgTAP file under `test/sql/` and a panel entry to `test/dogfood/panel.sh` for scaling validation.

## TDD

Tests describe the contract before the implementation. Cycle: red test → minimal impl → green → next cycle.

- **Pure-Rust** : `cargo test` for `core/` and `lang/`. Tests inline in `#[cfg(test)] mod tests` next to the code under test — standard Rust convention, access to private items without ceremony.
- **SQL surface** : `pg/` is tested via **pgTAP**, files in `test/sql/*.sql`, runner `./test/run.sh` against the PG17 instance managed by pgrx. No `pgrx-tests` / `#[pg_test]` — SQL is tested in SQL.
- **Iteration loop** : `cargo check --features pg17 --no-default-features` before `cargo pgrx install` — surfaces FFI errors in seconds; install is the slow last mile. The pgTAP runner recreates the DB but does NOT reinstall the extension; install first.
- **Visibility for cross-layer constants** : `core/` items consumed by `pg/` need `pub(crate)`, not `pub(super)`. The `core::moniker::encoding` constants (`VERSION`, `HEADER_FIXED_LEN`, `read_u16`, `write_u16`) are the canonical example.

## Workflow

Canonical loop after a non-trivial change:

```bash
cargo check --features pg17 --no-default-features --tests   # FFI/lifetime check, seconds
cargo test  --features pg17 --no-default-features --lib     # unit tests, sub-second
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
./test/run.sh                                               # pgTAP suite, ~5s
./test/dogfood.sh --only <project>                          # scaling validation
```

The dogfood runner clones the panel into `/dogfood/` (gitignored) on first use; subsequent runs reuse the clones unless `--reset` is passed.

Bench at scale via `cargo run --release --example bench_codegraph` (CodeGraph throughput) or `cargo run --release --example bench_extract` (full extractor on a real file).

## pgrx 0.18 manual Datum

`moniker` ships its bytes as a raw varlena, not the default cbor wrapper. To keep `#[derive(PostgresType)]` for the SQL DDL emission while replacing the cbor `IntoDatum`/`FromDatum`, use the opt-out attribute `#[bikeshed_postgres_type_manually_impl_from_into_datum]` and provide the five impls manually (`IntoDatum`, `FromDatum`, `BoxRet`, `UnboxDatum`, `ArgAbi`). The macro source at `pgrx-macros-0.18.0/src/lib.rs:902-973` is the canonical reference for the shape — mirror it, swap cbor encode/decode for varlena helpers (`pgrx::set_varsize_4b`, `pgrx::varlena_to_byte_slice`, `pg_sys::pg_detoast_datum_packed`).

**GIN bulk-build trap.** `rust_regtypein("X")` raises `type "X" does not exist` under restricted search_path (`CREATE INDEX USING gin (fn(graph))` over existing rows). Cache the OID in `OnceLock`, look up via `get_extension_oid` → `get_extension_schema` → `get_namespace_name` → `regtypein("schema.X")`. See `moniker_type_oid` in `src/pg/moniker/mod.rs`.

**Adding a `#[pg_extern]` arg without breaking callers**: wrap the new param in `pgrx::default!(T, "sql_literal")`. Existing SQL callsites stay valid; opt in via named arg (`fn extract_rust(... , deep := true)`).

## tree-sitter-rust gotchas

- Node kinds are `function_item` / `type_item` / `enum_item` / `trait_item` (not `fn_item` / `type_alias_item`).
- Closure `parameters` field is `closure_parameters`; children are bare patterns (`|x|`) OR `parameter` wrappers (`|x: i32|`). Counting only `kind == "parameter"` undercounts untyped closures.
- Statement-position `if_expression` / `match_expression` is wrapped in `expression_statement`. A body-walker dropping that kind loses locals nested in `if cond { let x = … }`.
