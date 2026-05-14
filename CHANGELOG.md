# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The three published crates share a single workspace version. Breaking
changes are allowed in minor releases as long as the project is in
`0.y.z`.

## [0.2.0] — 2026-05-13

### Added

- **`code-moniker-core`** — `CanonicalWalker` collapses adjacent
  same-kind comment nodes into a single `comment` def spanning the
  block; `lines = N` now reflects the block, not 1 per line.
- **CLI rule pack** — `rust.comment.comment-max-lines` caps `///` /
  `//` blocks at 4 lines; module-level `//!` and `SAFETY:` narratives
  exempt.
- **`code-moniker check`** — `// code-moniker: ignore[<id>]` now also
  suppresses violations on the comment def carrying the directive.
- **CLI** — `code-moniker manifest <PATH>` extracts declared deps from
  `Cargo.toml`, `package.json`, `pom.xml`, `pyproject.toml`, `go.mod`,
  `*.csproj` (auto-detected, or walk a directory). Emits one row per
  dep with `package_moniker` byte-identical to extractor `external_pkg:`
  heads, so consumers `@>`-join. Formats: tsv (default), json, tree.
- **`code-moniker-core`** — `lang::build_manifest` unifies the six
  per-lang manifest parsers behind a `Manifest` enum + filename
  dispatcher; preserves per-lang splitting (TS scopes, Go slashes,
  C# dots). Each `lang::*::build` exposes `package_moniker(project,
  import_root)`.
- **PG** — `package_moniker moniker` column on `extract_cargo`,
  `extract_package_json`, `extract_pom_xml`, `extract_pyproject`,
  `extract_go_mod`, `extract_csproj`. Signature now
  `(anchor moniker, content text)`.
- **CLI** — subcommand-first surface: `extract`, `check`, `langs
  [TAG]`, `shapes`. Every operation is an explicit verb.
- **CLI** — `extract --shape <SHAPE>` (`namespace`, `type`,
  `callable`, `value`, `annotation`, `ref`); repeatable or
  comma-separated; ANDs with `--kind`. `--kind` / `--shape` also
  accept comma-separated lists.
- **CLI** — `extract --format tree` renders a moniker-segment outline
  with refs nested under their source def. `--color auto|always|never`
  honors `NO_COLOR` / `CLICOLOR` / `TERM=dumb`; `--charset utf8|ascii`.
  Default filter drops `local`/`param`/`comment` and hides refs; any
  `--kind` re-enables full output. Behind the `pretty` cargo feature
  (default on).
- **`code-moniker-core`** — `Shape::Ref` variant, `Shape::ALL`,
  `Shape::for_kind`, `Shape: FromStr`.

### Changed

- **CLI** — `--format tree` no longer truncates signatures (was a
  32-char ellipsis).
- **CLI library** — `ExtractArgs` / `CheckArgs` rename `file` →
  `path` (the arg accepts directories too).
- **Scripts** — cleanup pass.
- **Docs** — refresh across `docs/cli/*`, `README.md`, `CLAUDE.md`,
  `CONTRIBUTING.md`, `docs/perf.md`. Agent-harness guide ships
  end-to-end recipes for six scenarios. `~600-line file cap` relaxed
  to a preference.

### Removed

- **CLI** — root-level `code-moniker <PATH>` form. Use
  `code-moniker extract <PATH>`; filters moved under the verb.

### Fixed

- **`code-moniker check`** — `target_lines_for` dropped the directive's
  line range when the next def had no position; now falls back to the
  directive's own lines.
- **pgTAP** — `00_smoke.sql` aligned to `pcm_version() = '0.2.0'`.
- **`code-moniker-core` (sql extractor)** — PL/pgSQL body re-parse
  leaked phantom `comment` defs with synthetic-buffer byte offsets onto
  the outer file; the inner `SELECT <expr>` walker now skips comments.
- **`code-moniker-core` (rs extractor)** — `use std::io::{self, Read,
  Write};` emitted N duplicate `imports_module → std::io` refs; dedup
  the parent-module ref per `use`. Per-leaf `imports_symbol` unaffected.
- **`code-moniker-core` (rs extractor)** — free-fn `calls` and self
  `method_call` to same-file callees now emit `confidence = resolved`
  (was `unresolved`); converges with Go / Python / C#.
- **`code-moniker-core` (ts extractor)** — same-file free-fn `calls`
  now emit `confidence = resolved` (was `name_match`). Covers three
  paths: top-level `function`, top-level `const X = anyExpr` (indexed
  with kind `const`), and nested `function` declarations (hoisted into
  the enclosing scope, matching JS semantics).
- **Docs** — `docs/cli/check.md` path encoding table: Go and C# use
  `package:<segment>/module:<stem>` (not `dir:`); SQL nests
  `schema:<name>` under `module:` (was reversed).
- **Docs** — `docs/cli/extract.md`: `--format json` is no longer
  equated to `code_graph_to_spec(graph)` — it's a distinct `{uri,
  lang, matches: {defs, refs}}` shape. Ref JSON example lists
  `binding`. Synopsis lists `--color` and `--charset`.
- **CI** — release workflow split for the workspace: publishes
  `code-moniker-core` then `code-moniker` in order, verifies git tag
  against the core crate. `code-moniker-pg` excluded from auto-publish.

## [0.1.0] — 2026-05-13

Initial public release of the three crates: `code-moniker-core` (pure
Rust foundation), `code-moniker` (standalone CLI + linter), and
`code-moniker-pg` (PostgreSQL extension via pgrx).
