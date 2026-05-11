# Contributing to `code-moniker`

Build, test, and extend the project. The conceptual model and SQL
surface are documented in [`docs/design/spec.md`](docs/design/spec.md).

## Layout

```
src/
  lib.rs                entry point, gates pgrx behind pgN features
  bin/code_moniker.rs   standalone CLI entry (feature `cli`)
  core/                 pure Rust, no pgrx, testable with `cargo test`
    moniker/            Moniker struct + Ord + tree-position queries
    uri/                typed canonical URI parse / serialize
    declare/            declarative spec lifecycle (jsonb ↔ code_graph)
    code_graph.rs       defs / refs / O(1) moniker→idx index
  cli/                  CLI internals (extract, check, presets)
  pg/                   pgrx wrappers, gated by pgN feature
    moniker/            moniker SQL type + opclasses (btree / hash / GiST)
    code_graph/         code_graph SQL type + accessors
    extract.rs          extract_<lang> SQL entries
    build.rs            extract_cargo / extract_package_json / ...
  lang/                 per-language extractors
    kinds.rs            cross-language vocabulary (VIS_* / CONF_* / kinds)
    extractor.rs        `LangExtractor` trait + default impls
    mod.rs              `define_languages!` macro (single dispatch table)
    ts/ rs/ java/ python/ go/ cs/ sql/
pgtap/
  run.sh                pgTAP harness (run via ./pgtap/run.sh)
  sql/                  pgTAP test files
scripts/
  check-arch.sh         dogfood the linter on src/
  dogfood.sh            multi-project ingestion runner
  dogfood/panel.sh      pinned panel of representative open-source projects
examples/
  bench_codegraph.rs    CodeGraph add_def / add_ref scaling bench
  bench_extract.rs      full extractor on a real file
```

**No file > ~600 lines.** One responsibility per file, named by its suffix.
When a file exceeds the cap, split the production module (subfiles
with their own `mod tests`); do not extract the tests.

## Workflow

```sh
cargo check --features pg17 --no-default-features --tests   # FFI/lifetime check, seconds
cargo test  --features pg17 --no-default-features --lib     # unit tests, sub-second
cargo clippy --features pg17 --no-default-features --tests --no-deps -- -D warnings
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
./pgtap/run.sh                                               # pgTAP suite, ~5s
./scripts/dogfood.sh --only <project>                          # scaling validation
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
changes, then `./scripts/check-arch.sh` (the `code-moniker check src/`
self-lint) when staged changes touch `src/*.rs`.

Project formatting convention: `hard_tabs = true` (`rustfmt.toml`).

## TDD

Cycle: red test → minimal impl → green.

- **Pure-Rust**: `cargo test` for `core/` and `lang/`. Tests inline in
  `#[cfg(test)] mod tests` next to the code under test.
- **SQL surface**: `pg/` is tested via **pgTAP**, files in
  `pgtap/sql/*.sql`, runner `./pgtap/run.sh` against the pgrx-managed
  PG17 instance. No `pgrx-tests` / `#[pg_test]`.
- **Iteration loop**: `cargo check --features pg17 --no-default-features`
  before `cargo pgrx install`. The pgTAP runner does NOT reinstall the
  extension — install first.
- **Cross-layer visibility**: `core/` items consumed by `pg/` need
  `pub(crate)`, not `pub(super)`. Canonical example:
  `core::moniker::encoding` constants (`VERSION`, `HEADER_FIXED_LEN`,
  `read_u16`, `write_u16`).

## Benchmarks

```sh
cargo run --release --features pg17 --no-default-features --example bench_codegraph
cargo run --release --features pg17 --no-default-features --example bench_extract
```

Dogfood runner clones the panel into `/dogfood/` (gitignored) on first
use; reuses on subsequent runs unless `--reset` is passed.

## Adding a language

A new extractor under `src/lang/<lang>/` mirrors the `ts/` skeleton:

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
  `src/pg/build.rs::extract_<system>`).

Adding a kind or visibility requires updating the trait constants
**and** `docs/declare_schema.json` (enforced by the schema-sync test
in `src/lang/mod.rs`).

Wire the SQL surface in `src/pg/extract.rs` (`#[pg_extern] fn
extract_<lang>(...)`); add a pgTAP file under `pgtap/sql/` and a panel
entry to `scripts/dogfood/panel.sh`.

The allowed kinds and visibilities per language are enumerated in
`src/lang/<lang>/mod.rs` (the `LangExtractor` trait constants) and
mirrored in `docs/declare_schema.json`.

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
in `src/pg/moniker/mod.rs`.

**Adding a `#[pg_extern]` arg without breaking callers**: wrap the new
param in `pgrx::default!(T, "sql_literal")`. Opt in via named arg
(`fn extract_rust(..., deep := true)`).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE),
at your option. Contributions are accepted under the same terms.
