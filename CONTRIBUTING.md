# Contributing to `code-moniker`

Build, test, and extend the project. The moniker identity format is documented
in [docs/design/moniker-uri.md](docs/design/moniker-uri.md).

## Layout

Cargo workspace, three crates:

- `code-moniker-core` — pure-Rust foundation, model, and language extractors.
- `code-moniker-workspace` — reusable workspace scan, cache, linkage, changes, and read models.
- `code-moniker` — standalone CLI / linter (`cargo install code-moniker`).

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
  workspace/                reusable workspace engine
    src/
      source/               source catalog and content loading
      code/                 graph indexing
      linkage/              cross-file linkage indexes and binding store
      registry/             runtime and local workspace registry
  cli/                      code-moniker (binary + lib)
    src/
      lib.rs main.rs args.rs check/ dir.rs extract.rs format.rs
      lang.rs lines.rs predicate.rs walk.rs
    tests/
      cli_e2e.rs cli_functional.rs
```

Prefer small files with one responsibility and a clear suffix. Some legacy
modules are larger; when a change grows one further, split the production
module around the responsibility being touched and keep the tests next to
the code under test.

## Workflow

```sh
cargo check  --workspace --all-targets
cargo test   --workspace
cargo clippy --workspace --all-targets --no-deps -- -D warnings
cargo arch-check                                                        # workspace-wide rule lint
```

## Pre-commit hook

```sh
git config core.hooksPath .githooks
```

Activates `.githooks/pre-commit`, which runs `cargo fmt --check` +
`cargo clippy ... -D warnings` on staged `*.rs` / `Cargo.{toml,lock}`
changes, then `cargo arch-check` (the workspace-wide self-lint) when
staged changes touch any `*.rs` under `crates/`.

Project formatting convention: `hard_tabs = true` (`rustfmt.toml`).

## TDD

Cycle: red test → minimal impl → green.

- **Rust workspace**: `cargo test --workspace` covers the core,
  workspace, and CLI crates. Tests live inline in `#[cfg(test)] mod tests`
  next to focused units, with CLI integration tests under `crates/cli/tests/`.
- **Cross-crate visibility**: items in `code_moniker_core` consumed by
  the workspace or CLI crates must be `pub` (not `pub(crate)`).

## Benchmarks

```sh
cargo run --release -p code-moniker-core --example bench_codegraph
cargo run --release -p code-moniker-core --example bench_extract
```

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
- `strategy.rs` — AST traversal, extractor state, def emitters, ref
  emitters, local-scope tracking, visibility helpers, and any language
  lookup tables. Use `RefAttrs { ..RefAttrs::default() }` when emitting refs;
  use `add_ref_attrs` when confidence, alias, binding, or receiver hints are
  known.
- Optional manifest parser yielding `Vec<Dep>` for CLI and workspace manifest
  commands.

Adding a kind or visibility requires updating the trait constants
and the tests that assert language discovery output.

The allowed kinds and visibilities per language are enumerated in
`crates/core/src/lang/<lang>/mod.rs` (the `LangExtractor` trait constants).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE),
at your option. Contributions are accepted under the same terms.
