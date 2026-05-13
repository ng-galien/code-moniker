# Contributing to `code-moniker`

Build, test, and extend the project. The conceptual model and SQL
surface are documented in the [spec](docs/design/spec.md).

## Layout

Cargo workspace, three crates published independently:

- `code-moniker-core` — pure-Rust foundation (core, lang, declare). No pgrx.
- `code-moniker` — standalone CLI / linter (`cargo install code-moniker`).
- `code-moniker-pg` — pgrx cdylib (PostgreSQL extension).

```
Cargo.toml                  workspace manifest
crates/
  core/                     code-moniker-core
    src/
      core/                 Moniker, URI, CodeGraph (pure Rust, no parser)
      declare/              jsonb ↔ code_graph lifecycle
      lang/                 per-language extractors
        kinds.rs, extractor.rs, mod.rs, ts/, rs/, java/, python/, go/, cs/, sql/
    examples/
      bench_codegraph.rs    CodeGraph add_def / add_ref scaling bench
      bench_extract.rs      full extractor on a real file
  cli/                      code-moniker (binary + lib)
    src/
      lib.rs main.rs args.rs check/ dir.rs extract.rs format.rs
      lang.rs lines.rs predicate.rs walk.rs
    tests/
      cli_e2e.rs cli_functional.rs
  pg/                       code-moniker-pg
    Cargo.toml              [lib] crate-type = ["cdylib", "rlib"]
    code_moniker.control    extension manifest (filename = SQL extension name)
    src/
      lib.rs                pgrx::pg_module_magic!
      moniker/              moniker SQL type + opclasses
      code_graph/           code_graph SQL type + accessors
      extract.rs            extract_<lang> SQL entries
      build.rs              extract_cargo / extract_package_json / ...
pgtap/
  run.sh                    pgTAP harness against the pgrx-managed PG17
  sql/                      pgTAP test files
scripts/
  check-arch.sh             dogfood the linter on the whole workspace
  dogfood.sh                multi-project ingestion runner
  dogfood/panel.sh          pinned panel of representative open-source projects
```

**No file > ~600 lines.** One responsibility per file, named by its suffix.
When a file exceeds the cap, split the production module (subfiles
with their own `mod tests`); do not extract the tests.

## Workflow

```sh
cargo check  --workspace --exclude code-moniker-pg --all-targets       # CLI + core
cargo test   --workspace --exclude code-moniker-pg                     # unit + integration
cargo clippy --workspace --exclude code-moniker-pg --all-targets --no-deps -- -D warnings
cargo check  -p code-moniker-pg --features pg17                        # FFI / lifetime check on pg
cargo clippy -p code-moniker-pg --features pg17 --no-deps -- -D warnings
cargo pgrx install --manifest-path crates/pg/Cargo.toml --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
./pgtap/run.sh                                                          # pgTAP suite, ~5s
./scripts/dogfood.sh --only <project>                                   # scaling validation
```

Initial pgrx setup (one-time, ~15 min):

```sh
cargo install --locked cargo-pgrx
cargo pgrx init --pg17 download
```

pgTAP install against the pgrx PG17 (one-time):

```sh
git clone --depth 1 https://github.com/theory/pgtap.git /tmp/pgtap-build
PG_CONFIG=$HOME/.pgrx/17.9/pgrx-install/bin/pg_config make -C /tmp/pgtap-build install
```

## Pre-commit hook

```sh
git config core.hooksPath .githooks
```

Activates `.githooks/pre-commit`, which runs `cargo fmt --check` +
`cargo clippy ... -D warnings` on staged `*.rs` / `Cargo.{toml,lock}`
changes, then `./scripts/check-arch.sh` (the workspace-wide
self-lint) when staged changes touch any `*.rs` under `crates/`.

Project formatting convention: `hard_tabs = true` (`rustfmt.toml`).

## TDD

Cycle: red test → minimal impl → green.

- **Pure-Rust**: `cargo test --workspace --exclude code-moniker-pg` for
  the `code_moniker_core` lib (core / lang / declare) plus the CLI.
  Tests inline in `#[cfg(test)] mod tests` next to the code under test.
- **SQL surface**: `code-moniker-pg` is tested via **pgTAP**, files in
  `pgtap/sql/*.sql`, runner `./pgtap/run.sh` against the pgrx-managed
  PG17 instance. No `pgrx-tests` / `#[pg_test]`.
- **Iteration loop**: `cargo check -p code-moniker-pg --features pg17`
  before `cargo pgrx install`. The pgTAP runner does NOT reinstall the
  extension — install first.
- **Cross-crate visibility**: items in `code_moniker_core` consumed by
  the CLI or PG crates must be `pub` (not `pub(crate)`). Canonical
  examples: `core::moniker::encoding` constants (`VERSION`,
  `HEADER_FIXED_LEN`, `read_u16`, `write_u16`),
  `Moniker::from_canonical_bytes`, `MonikerView::from_canonical_bytes`.

## Benchmarks

```sh
cargo run --release -p code-moniker-core --example bench_codegraph
cargo run --release -p code-moniker-core --example bench_extract
```

Dogfood runner clones the panel into `/dogfood/` (gitignored) on first
use; reuses on subsequent runs unless `--reset` is passed.

## Adding a language

A new extractor under `crates/core/src/lang/<lang>/` mirrors the `ts/` skeleton:

- `mod.rs` — `pub fn parse`, `pub fn extract(uri, source, anchor, deep, &Presets) -> CodeGraph`,
  `pub struct Presets` for caller-supplied hints. Plus a zero-sized
  `pub struct Lang;` implementing `lang::LangExtractor` with
  `LANG_TAG`, `ALLOWED_KINDS`, `ALLOWED_VISIBILITIES`, forwarding
  `extract` to the free function.
- `kinds.rs` — language-specific structural kinds + `pub(super) use crate::lang::kinds::{VIS_*, CONF_*}`
  for the shared vocabulary. Never redeclare visibility or confidence values.
- `canonicalize.rs` — `compute_module_moniker`, `extend_segment`,
  `extend_callable` with arity-based segment names.
- `walker.rs` — Walker struct (source bytes, module, deep, presets,
  scope state, language-specific tables) + AST dispatch + def emitters.
- `refs.rs` — ref emitters per kind. Use `RefAttrs { ..RefAttrs::default() }`
  shorthand. `add_ref_attrs` for confidence / alias / receiver_hint;
  bare `add_ref` only when nothing is known.
- `scope.rs` — local-scope stack (`record_local`, `is_local_name`,
  `name_confidence`) and language-specific visibility helper. Push /
  pop on each callable so `confidence: local` stays accurate.
- Optional `imports.rs` (when imports decompose into many specifiers)
  and `build.rs` (manifest parser yielding `Vec<Dep>` consumed by
  `crates/pg/src/build.rs::extract_<system>`).

Adding a kind or visibility requires updating the trait constants
**and** `docs/postgres/declare-schema.json` (enforced by the schema-sync test
in `crates/core/src/lang/mod.rs`).

Wire the SQL surface in `crates/pg/src/extract.rs` (`#[pg_extern] fn
extract_<lang>(...)`); add a pgTAP file under `pgtap/sql/` and a panel
entry to `scripts/dogfood/panel.sh`.

The allowed kinds and visibilities per language are enumerated in
`crates/core/src/lang/<lang>/mod.rs` (the `LangExtractor` trait
constants) and mirrored in `docs/postgres/declare-schema.json`.

## pgrx 0.18 gotchas

`moniker` ships bytes as a raw varlena, not cbor. Uses
`#[bikeshed_postgres_type_manually_impl_from_into_datum]` with the
five impls (`IntoDatum`, `FromDatum`, `BoxRet`, `UnboxDatum`, `ArgAbi`)
written by hand. Reference shape: `pgrx-macros-0.18.0/src/lib.rs:902-973`.
Varlena helpers: `pgrx::set_varsize_4b`, `pgrx::varlena_to_byte_slice`,
`pg_sys::pg_detoast_datum_packed`.

**GIN bulk-build trap.** `rust_regtypein("X")` raises `type "X" does not
exist` under restricted search_path. Cache the OID in `OnceLock` and
look it up via `get_extension_oid` → `get_extension_schema` →
`get_namespace_name` → `regtypein("schema.X")`. See `moniker_type_oid`
in `crates/pg/src/moniker/mod.rs`.

**Adding a `#[pg_extern]` arg without breaking callers**: wrap the new
param in `pgrx::default!(T, "sql_literal")`. Opt in via named arg
(`fn extract_rust(..., deep := true)`).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE),
at your option. Contributions are accepted under the same terms.
