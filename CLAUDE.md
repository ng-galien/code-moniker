# CLAUDE.md

`code-moniker` — PostgreSQL extension in Rust + pgrx + standalone CLI (`code-moniker`). Native `moniker` and `code_graph` types with GiST-indexed algebra. No tables, no triggers, no persistent state — **core + operators + per-language extractors**.

## Documents

- `README.md` — posture, two-pitch entry point, status
- `CONTRIBUTING.md` — developer workflow (build, test, dogfood, add a language)
- `CLAUDE.md` (this file) — coding rules
- `docs/README.md` — single doc index (no per-category READMEs)
- `docs/cli/extract.md`, `docs/cli/check.md`, `docs/cli/check-dsl.md`, `docs/cli/langs.md`, `docs/cli/agent-harness.md` — CLI surface
- `docs/postgres/reference.md`, `docs/postgres/usage.md` — PostgreSQL extension
- `docs/design/spec.md`, `docs/design/moniker-uri.md` — conceptual model + URI grammar
- `docs/postgres/declare-schema.json` — JSON Schema 2020-12 for `code_graph_declare`. Per-language profiles must stay in sync with `LangExtractor::ALLOWED_KINDS` / `ALLOWED_VISIBILITIES` (enforced by the schema-sync test in `crates/core/src/lang/mod.rs`).

## Comment sobriety

- **Code**: minimal comments. No narrating what the code does — names and flow already tell that. No essays in module `//!` docstrings: a short paragraph suffices.
- **Tests**: this is the legitimate place for documentation. A short description of the invariant tested is welcome. The test name is the spec (`extract_simple_class_emits_class_def`).
- No emoji. No "smart" framing. Sober, technical.

## Layout

Cargo workspace, three crates published independently:

- `code-moniker-core` (lib) — pure Rust foundation. No pgrx, no opt deps.
- `code-moniker` (binary `code-moniker` + lib) — standalone CLI / linter.
- `code-moniker-pg` (pgrx cdylib) — PostgreSQL extension.

```
Cargo.toml                  workspace manifest (members + shared deps)
crates/
  core/                     code-moniker-core
    src/
      lib.rs                pub mod core / declare / lang
      core/                 pure-Rust foundation
        moniker/            Moniker struct + Ord + tree-position queries
                            (mod, encoding, view, builder, query)
        uri/                typed canonical URI parse / serialize
        code_graph.rs       defs / refs / O(1) moniker→idx, DefAttrs / RefAttrs
      declare/              declarative spec lifecycle (jsonb ↔ code_graph).
                            Sits above core/ because DeclareSpec.lang: Lang
                            carries the lang enum from lang/.
      lang/                 per-language extractors
        kinds.rs            cross-language vocabulary (VIS_* / CONF_* / kinds)
        extractor.rs        LangExtractor trait + assert_conformance helper
        mod.rs              define_languages! dispatch table
        ts/ rs/ java/ python/ go/ cs/ sql/
    examples/
      bench_codegraph.rs    CodeGraph add_def / add_ref scaling bench
      bench_extract.rs      full extractor on a real file
  cli/                      code-moniker (CLI + lib)
    Cargo.toml              [[bin]] code-moniker, [lib] code_moniker_cli
    src/
      lib.rs                pub Cli / run / OutputFormat / etc.
      main.rs               clap entry, delegates to lib
      args.rs check/ dir.rs extract.rs format.rs lang.rs
      lines.rs predicate.rs walk.rs
    tests/
      cli_e2e.rs            in-process: drives lib::run with captured writers
      cli_functional.rs     subprocess: real binary, real stdio
  pg/                       code-moniker-pg (pgrx cdylib)
    Cargo.toml              [lib] crate-type = ["cdylib", "rlib"]
    code_moniker.control    extension manifest (filename = SQL extension name)
    src/
      lib.rs                pgrx::pg_module_magic!, mod gates
      moniker/              moniker SQL type + operators
      code_graph/           code_graph SQL type + accessors
      extract.rs            extract_<lang> SQL entries
      build.rs              manifest extractors (extract_cargo / package_json / …)
pgtap/
  run.sh                    pgTAP harness against the pgrx-managed PG
  coverage.sh               cargo --lib + pgTAP under llvm-cov
  sql/                      pgTAP test files
scripts/
  check-arch.sh             dogfood the linter on the whole workspace
  dogfood.sh                multi-project ingestion runner
  dogfood/panel.sh          pinned panel of representative open-source projects
```

**No file > ~600 lines.** One responsibility per file, named by its suffix. When a file exceeds the cap, split the production module (subfiles with their own `mod tests`); do not extract the tests.

## Extractor extension protocol

A new language under `crates/core/src/lang/<lang>/` mirrors the `ts/` skeleton:

- `mod.rs` — `pub fn parse`, `pub fn extract(uri, source, anchor, deep, &Presets) -> CodeGraph`, `pub struct Presets` for caller-supplied hints.
- `kinds.rs` — language-specific structural kinds + `pub(super) use crate::lang::kinds::{VIS_*, CONF_*}` for the shared vocabulary. Never redeclare visibility or confidence values.
- `canonicalize.rs` — `compute_module_moniker`, `extend_segment`, `extend_callable` with arity-based segment names.
- `walker.rs` — Walker struct (source bytes, module, deep, presets, scope state, language-specific tables like `imports` / `type_table`) + AST dispatch + def emitters.
- `refs.rs` — ref emitters per kind. Use `RefAttrs { ..RefAttrs::default() }` shorthand so future fields land without touching every site. Reach for `add_ref_attrs` when emitting confidence / alias / receiver_hint; the bare `add_ref` is for cases where nothing is known.
- `scope.rs` — local-scope stack (`record_local`, `is_local_name`, `name_confidence`) and language-specific visibility helper. Defaults differ per language — Java is `package`, TS is `public`. Push/pop on each callable so `confidence: local` stays accurate.
- Optional `imports.rs` (when imports decompose into many specifiers) and `build.rs` (manifest parser yielding `Vec<Dep>` consumed by `crates/pg/src/build.rs::extract_<system>`).

- **Pre-pass `collect_type_table`** (when methods can precede their receiver type, e.g. Go / Rust impl): emits top-level type defs to the graph AND fills the resolution HashMap. Walker's `handle_type_spec` gates `add_def_attrs` on `scope != module` to skip the duplicate. `DuplicateMoniker` is silently tolerated everywhere via `let _ = graph.add_def(...)`.

- **`ParentNotFound` / `SourceNotFound` are silently dropped** by `let _ = graph.add_def_attrs(...)`. Synthesizing a moniker whose parent isn't in the graph drops every child (commit `9c23d04` Rust impl-for-external). Pre-check via `graph.contains(&parent)`; if missing, emit a placeholder first with `origin = ORIGIN_EXTRACTED` — `assert_conformance` rejects `ORIGIN_INFERRED` / `ORIGIN_DECLARED` from extractors (reserved for `code_graph_declare`).

Every new extractor MUST implement `lang::LangExtractor` on a zero-sized `pub struct Lang;` at the top of `crates/core/src/lang/<lang>/mod.rs`, exposing `LANG_TAG`, `ALLOWED_KINDS`, `ALLOWED_VISIBILITIES`, and forwarding `extract` to the free function. `extract_default` test helper calls `lang::assert_conformance::<Lang>(&g, anchor)` on every fixture. Adding a kind or visibility requires updating the trait constants AND `docs/postgres/declare-schema.json`.

Wire the SQL surface in `crates/pg/src/extract.rs` (`#[pg_extern] fn extract_<lang>(...)`); add a pgTAP file under `pgtap/sql/` and a panel entry to `scripts/dogfood/panel.sh`.

## TDD

Cycle: red test → minimal impl → green.

- **Pure-Rust** : `cargo test --workspace --exclude code-moniker-pg` for `core/`, `lang/`, and the CLI. Tests inline in `#[cfg(test)] mod tests` next to the code under test.
- **SQL surface** : the `code-moniker-pg` crate is tested via **pgTAP**, files in `pgtap/sql/*.sql`, runner `./pgtap/run.sh` against the pgrx-managed PG17 instance. No `pgrx-tests` / `#[pg_test]`.
- **Iteration loop** : `cargo check -p code-moniker-pg --features pg17` before `cargo pgrx install`. The pgTAP runner does NOT reinstall the extension — install first.
- **Cross-crate visibility** : `code_moniker_core` items consumed by the CLI or PG crates need `pub`, not `pub(crate)`. Canonical examples: `core::moniker::encoding` constants (`VERSION`, `HEADER_FIXED_LEN`, `read_u16`, `write_u16`), `Moniker::from_canonical_bytes`, `MonikerView::from_canonical_bytes`.

## Workflow

```bash
cargo check  --workspace --exclude code-moniker-pg --all-targets       # CLI + core, fast
cargo test   --workspace --exclude code-moniker-pg                     # unit + integration
cargo clippy --workspace --exclude code-moniker-pg --all-targets --no-deps -- -D warnings
cargo check  -p code-moniker-pg --features pg17                        # FFI / lifetime check on pg
cargo clippy -p code-moniker-pg --features pg17 --no-deps -- -D warnings
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config --manifest-path crates/pg/Cargo.toml
./pgtap/run.sh                                               # pgTAP suite, ~5s
./scripts/dogfood.sh --only <project>                          # scaling validation
./scripts/dogfood-check.sh                                     # asserts per-(project,kind) floors in scripts/dogfood/baselines.tsv
./scripts/dogfood-baseline.sh                                  # regen floors after a legitimate count change; commit the TSV diff
```

`dogfood.sh` drops + recreates `pcm_dogfood` on each run, including with `--lang <x>` (wipes other-lang data). Commit baseline diffs alongside extractor changes — counts moving up = fix worked; silent down = regression `dogfood-check.sh` would have caught.

Pre-commit hook runs `cargo fmt -- --check` + `cargo clippy ... -D warnings` on `*.rs` / `Cargo.{toml,lock}` changes. Clippy lints (`manual_find`, `manual_let_else`) block the commit — run it proactively.

Dogfood runner clones the panel into `/dogfood/` (gitignored) on first use; reuses on subsequent runs unless `--reset` is passed.

Bench: `cargo run --release -p code-moniker-core --example bench_codegraph` (CodeGraph throughput), `cargo run --release -p code-moniker-core --example bench_extract` (full extractor on a real file).

## pgrx 0.18 manual Datum

`moniker` ships bytes as a raw varlena, not cbor. Use `#[bikeshed_postgres_type_manually_impl_from_into_datum]` and provide the five impls manually (`IntoDatum`, `FromDatum`, `BoxRet`, `UnboxDatum`, `ArgAbi`). Reference shape: `pgrx-macros-0.18.0/src/lib.rs:902-973`. Varlena helpers: `pgrx::set_varsize_4b`, `pgrx::varlena_to_byte_slice`, `pg_sys::pg_detoast_datum_packed`.

**GIN bulk-build trap.** `rust_regtypein("X")` raises `type "X" does not exist` under restricted search_path (`CREATE INDEX USING gin (fn(graph))` over existing rows). Cache the OID in `OnceLock`, look up via `get_extension_oid` → `get_extension_schema` → `get_namespace_name` → `regtypein("schema.X")`. See `moniker_type_oid` in `crates/pg/src/moniker/mod.rs`.

**Adding a `#[pg_extern]` arg without breaking callers**: wrap the new param in `pgrx::default!(T, "sql_literal")`. Opt in via named arg (`fn extract_rust(..., deep := true)`).

pgrx-pg-sys and `libpg_query` (`pg_query` crate) cannot cohabit a binary; both define `palloc`, `MemoryContext`, `plpgsql_check_syntax`. SQL / PL-pgSQL must stay on tree-sitter-postgres.

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
- Grouped `var ( ... )` and `const ( ... )` wrap specs in an extra `var_spec_list` / `const_spec_list` layer; single-line `var x int` puts the spec directly under `var_declaration`. Flatten both — see `spec_children` in `src/lang/go/strategy.rs`.

## tree-sitter-postgres gotchas

- Two grammars in one crate: `tree_sitter_postgres::LANGUAGE` for SQL top-level (`CreateFunctionStmt`, `CreateStmt`, `ViewStmt`, `SelectStmt`, …) and `tree_sitter_postgres::LANGUAGE_PLPGSQL` for function bodies. Pure-Rust, no postmaster, no symbol clash with pgrx — both can be linked in the same binary as the PG extension.
- `func_arg_list` is **left-recursive**: a call `f(a, b, c)` produces `func_arg_list(func_arg_list(func_arg_list(a), b), c)`. Counting arity by listing direct children of the outer list only catches the last arg; walk recursively over the subtree for every `func_arg_expr`.
- `func_application` is the call node; `func_name` is its first named child. The same `func_name` shape is reused by `CreateFunctionStmt` and `CallStmt`.
- `qualified_name` / `func_name` decompose `schema.name` as `ColId(name)` then optional `indirection > indirection_el > attr_name`. Walk into both `ColId` and `attr_name` for the identifier leaf — top-level `identifier` children alone undercount.
- `func_type` returns raw text including `pg_catalog.` prefixing on keyword aliases (`int` becomes `pg_catalog.int4`). Strip the prefix and canonicalise (`int → int4`, `bigint → int8`, …) before using as the moniker signature.
- PL/pgSQL `sql_expression` is **opaque text** — the grammar parses PERFORM / IF / EXECUTE / `:=` envelopes but doesn't descend into the embedded SQL. To find `func_application` refs inside the expression, slice the text and re-parse with the SQL grammar (wrapping in `SELECT <expr>` if it's a bare expression).
- `CREATE FUNCTION` body is wrapped in a `dollar_quoted_string` under `func_as`. The delimiters are `$$` or `$tag$…$tag$` — strip them by finding the first and last occurrence of the delimiter run.
- `lang::callable::normalize_type_text` strips all whitespace; SQL keeps its own `normalize_type` (collapses runs to single spaces, applies `int → int4` aliases) to preserve `double precision` etc.
- `columnDef` (CREATE TABLE column declarations) is **lowercase `c`**, unlike most PG node kinds. Match on `"columnDef"` exactly; iterate via `visit(create_stmt, |n| n.kind() == "columnDef")` then read its `Typename` child.

## tree-sitter-python gotchas

- Docstrings (`"""..."""` as first stmt of a function/class/module body) are `expression_statement > string` (or `concatenated_string`), NOT `comment` nodes. Captured via `on_symbol_emitted` on FUNCTION/METHOD/CLASS + a post-walk pass on `tree.root_node()` in `extract()` for module-level — see `src/lang/python/strategy.rs::first_docstring`.

## Architectural rule authoring (`.code-moniker.toml`)

- Rust import refs encode `crate::X::Y::Z` (depth ≥3) as `dir:X/module:Y/path:Z`, `crate::X::Y` (depth 2) as `module:X/path:Y`. Path-based ref rules use `target ~ '**/dir:X/**' OR target ~ '**/module:X/**'`.
- Validate new path-rules by injection: `sed -i '' '1a use crate::FORBIDDEN as _' file.rs`, run linter, revert. `0 violations` baseline does not validate a rule.
- `scripts/check-arch.sh` runs `code-moniker check .` (whole repo, not `src/`).
- `// code-moniker: ignore[<id>]` applies to the next non-comment def. Comment-on-comment suppression is not supported; use rule-level `text =~ X` exemption.

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
