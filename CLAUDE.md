# CLAUDE.md

`code-moniker` — PostgreSQL extension in Rust + pgrx + standalone CLI (`code-moniker`). Native `moniker` and `code_graph` types with GiST-indexed algebra. No tables, no triggers, no persistent state — **core + operators + per-language extractors**.

## Documents

- `README.md` — posture, two-pitch entry point, status
- `CONTRIBUTING.md` — developer workflow (build, test, dogfood, add a language)
- `CLAUDE.md` (this file) — coding rules
- `docs/README.md` — doc index, grouped by audience
- `docs/use-as-agent-harness.md`, `docs/use-in-postgres.md` — user guides
- `docs/cli-extract.md`, `docs/cli-check.md`, `docs/check-dsl.md` — CLI reference
- `docs/design/spec.md` — conceptual model, public API
- `docs/design/moniker-uri.md` — moniker URI grammar
- `docs/declare_schema.json` — JSON Schema 2020-12 for `code_graph_declare`. Per-language profiles must stay in sync with `LangExtractor::ALLOWED_KINDS` / `ALLOWED_VISIBILITIES` (enforced by the schema-sync test in `src/lang/mod.rs`).

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
    extract.rs          extract_typescript / extract_rust / extract_java /
                        extract_python / extract_go / extract_csharp /
                        extract_plpgsql SQL entries
    build.rs            extract_cargo / extract_package_json / extract_pom_xml /
                        extract_pyproject / extract_go_mod / extract_csproj
  lang/                 per-language extractors
    kinds.rs            cross-language vocabulary (VIS_* / CONF_* / structural
                        kind constants). New extractors `pub use` from here;
                        never redeclare.
    extractor.rs        `LangExtractor` trait (per-language contract), default
                        impls for `declare` / `to_spec`, `assert_conformance`
                        test-only check.
    mod.rs              `define_languages!` macro: single dispatch table that
                        generates `Lang` enum + `from_tag` / `tag` /
                        `allowed_kinds` / `allowed_visibilities` / `Lang::ALL`.
                        One line per supported language; build fails on omission.
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
    python/             Python (mod / kinds / canonicalize / walker / refs / scope /
                        build for pyproject.toml)
    go/                 Go (mod / kinds / canonicalize / walker / refs / scope /
                        build for go.mod)
    cs/                 C# (mod / kinds / canonicalize / walker / refs / scope /
                        build for .csproj)
    sql/                PL/pgSQL via PG runtime parser + vendored plpgsql sources (mod / kinds /
                        canonicalize / walker / body / refs / scope)
pgtap/
  run.sh                pgTAP harness against the pgrx-managed PG
  coverage.sh           cargo --lib + pgTAP under llvm-cov instrumentation
  sql/                  pgTAP test files
scripts/
  check-arch.sh         dogfood the linter on src/ (pre-commit + CI gate)
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

- **Pre-pass `collect_type_table`** (when methods can precede their receiver type, e.g. Go / Rust impl): emits top-level type defs to the graph AND fills the resolution HashMap. Walker's `handle_type_spec` gates `add_def_attrs` on `scope != module` to skip the duplicate. `DuplicateMoniker` is silently tolerated everywhere via `let _ = graph.add_def(...)`.
- **Triplicated helpers** (`resolve_type_target`, `stdlib_or_imported`, external-package target builders) are deliberately copy-pasted across `java/`, `python/`, `go/`. Do not factor prematurely.

Every new extractor MUST implement `lang::LangExtractor` on a zero-sized `pub struct Lang;` at the top of `src/lang/<lang>/mod.rs`, exposing `LANG_TAG`, `ALLOWED_KINDS`, `ALLOWED_VISIBILITIES`, and forwarding `extract` to the free function. `extract_default` test helper calls `lang::assert_conformance::<Lang>(&g, anchor)` on every fixture. Adding a kind or visibility requires updating the trait constants AND `docs/declare_schema.json`.

Wire the SQL surface in `src/pg/extract.rs` (`#[pg_extern] fn extract_<lang>(...)`); add a pgTAP file under `pgtap/sql/` and a panel entry to `scripts/dogfood/panel.sh`.

## TDD

Cycle: red test → minimal impl → green.

- **Pure-Rust** : `cargo test` for `core/` and `lang/`. Tests inline in `#[cfg(test)] mod tests` next to the code under test.
- **SQL surface** : `pg/` is tested via **pgTAP**, files in `pgtap/sql/*.sql`, runner `./pgtap/run.sh` against the pgrx-managed PG17 instance. No `pgrx-tests` / `#[pg_test]`.
- **Iteration loop** : `cargo check --features pg17 --no-default-features` before `cargo pgrx install`. The pgTAP runner does NOT reinstall the extension — install first.
- **Cross-layer visibility** : `core/` items consumed by `pg/` need `pub(crate)`, not `pub(super)`. Canonical example: `core::moniker::encoding` constants (`VERSION`, `HEADER_FIXED_LEN`, `read_u16`, `write_u16`).

## Workflow

```bash
cargo check --features pg17 --no-default-features --tests   # FFI/lifetime check, seconds
cargo test  --features pg17 --no-default-features --lib     # unit tests, sub-second
cargo clippy --features pg17 --no-default-features --tests --no-deps -- -D warnings
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
./pgtap/run.sh                                               # pgTAP suite, ~5s
./scripts/dogfood.sh --only <project>                          # scaling validation
```

Pre-commit hook runs `cargo fmt -- --check` + `cargo clippy ... -D warnings` on `*.rs` / `Cargo.{toml,lock}` changes. Clippy lints (`manual_find`, `manual_let_else`) block the commit — run it proactively.

Dogfood runner clones the panel into `/dogfood/` (gitignored) on first use; reuses on subsequent runs unless `--reset` is passed.

Bench: `cargo run --release --example bench_codegraph` (CodeGraph throughput), `cargo run --release --example bench_extract` (full extractor on a real file).

## pgrx 0.18 manual Datum

`moniker` ships bytes as a raw varlena, not cbor. Use `#[bikeshed_postgres_type_manually_impl_from_into_datum]` and provide the five impls manually (`IntoDatum`, `FromDatum`, `BoxRet`, `UnboxDatum`, `ArgAbi`). Reference shape: `pgrx-macros-0.18.0/src/lib.rs:902-973`. Varlena helpers: `pgrx::set_varsize_4b`, `pgrx::varlena_to_byte_slice`, `pg_sys::pg_detoast_datum_packed`.

**GIN bulk-build trap.** `rust_regtypein("X")` raises `type "X" does not exist` under restricted search_path (`CREATE INDEX USING gin (fn(graph))` over existing rows). Cache the OID in `OnceLock`, look up via `get_extension_oid` → `get_extension_schema` → `get_namespace_name` → `regtypein("schema.X")`. See `moniker_type_oid` in `src/pg/moniker/mod.rs`.

**Adding a `#[pg_extern]` arg without breaking callers**: wrap the new param in `pgrx::default!(T, "sql_literal")`. Opt in via named arg (`fn extract_rust(..., deep := true)`).

## tree-sitter-rust gotchas

- Node kinds are `function_item` / `type_item` / `enum_item` / `trait_item` (not `fn_item` / `type_alias_item`).
- Closure `parameters` field is `closure_parameters`; children are bare patterns (`|x|`) OR `parameter` wrappers (`|x: i32|`). Counting only `kind == "parameter"` undercounts untyped closures.
- Statement-position `if_expression` / `match_expression` is wrapped in `expression_statement`. A body-walker dropping that kind loses locals nested in `if cond { let x = … }`.

## tree-sitter-go gotchas

- `parameter_declaration` carries multiple identifier children sharing one `type` field (`func f(a, b int)`); count names and emit one type slot per name.
- `method_declaration` field `receiver` is a `parameter_list` with a single `parameter_declaration`; strip `pointer_type` and unwrap `generic_type.type` to recover the receiver type name.
- `import_spec` has fields `path` (interpreted_string_literal — strip `"`/backticks) and `name` (optional: `package_identifier` alias, or `dot`/`blank_identifier` for `. "fmt"` / `_ "fmt"`).
- `qualified_type` exposes prefix as field `package` and type as field `name` (not `path`).
- `composite_literal` has fields `type` and `body` (= `literal_value`); recurse on `generic_type.type` to peel `Foo[T]{}`.
- `short_var_declaration` / `var_declaration` / `range_clause` use field `left` (identifier OR expression_list); skip `_` blank patterns.

## tree-sitter-c-sharp gotchas

- Modifiers are wrapped in singular `modifier` nodes whose child is the keyword node (`public` / `private` / `internal` / `protected`); descend two levels to read visibility.
- `params object[] args` is flattened into `parameter_list` (no wrapping `parameter` node) — detect the `params` direct child of the parameter_list and emit `...` after the regular `parameter` slots.
- `record_declaration` does NOT expose field `parameters` or `body`; locate `parameter_list` and `declaration_list` via `named_children` lookup.
- `using_directive` has no clean field for the imported path: walk children and pick the first `qualified_name`/`identifier` that's not the alias `name` field. `static`, `global`, `dot`/`_` aliases are direct children of the directive.
- `member_access_expression` fields are `expression` (the receiver) and `name` (the member identifier); chained `foo().bar()` puts an `invocation_expression` in `expression` (use `HINT_CALL`).
- `attribute_list` is a direct child of the annotated declaration (class / method / property / field); each `attribute` has field `name` (identifier or qualified_name) and optional `attribute_argument_list` via field `arguments`.
- `local_declaration_statement` reuses `variable_declaration` (field `type` + `variable_declarator` children with field `name`); same shape as `field_declaration`.
- `foreach_statement` exposes a single iter var via field `left` (identifier, not `expression_list`), with separate fields `type`, `right`, `body`. `implicit_type` covers `var` — emit_uses_type can ignore it via the `predefined_type`/`implicit_type` skip arms.
- C# does NOT distinguish base class from interfaces in `base_list` syntactically — emit all entries as `EXTENDS`; consumers refine via cross-file resolution against def kinds.
- Top-level type default visibility is `internal` (= `VIS_PACKAGE`); class member default is `private`. Caller decides via parametrized `modifier_visibility(node, default)`.
