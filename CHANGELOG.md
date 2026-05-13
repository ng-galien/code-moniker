# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The three published crates share a single workspace version. Breaking
changes are allowed in minor releases as long as the project is in
`0.y.z`.

## [0.2.0] — 2026-05-13

### Added

- **CLI** — subcommand-first surface. Every operation is now an explicit
  verb:
  - `code-moniker extract <PATH>` — graph extraction (was the implicit
    default action at the root level).
  - `code-moniker check <PATH>` — project linter (unchanged).
  - `code-moniker langs [TAG]` — list supported languages, or kinds of one
    grouped by shape with allowed visibilities.
  - `code-moniker shapes` — cross-language shape vocabulary.
- **CLI** — `extract --shape <SHAPE>` filter by kind family
  (`namespace`, `type`, `callable`, `value`, `annotation`, `ref`),
  repeatable or comma-separated. ANDs with `--kind`.
- **CLI** — `--kind` and `--shape` accept comma-separated lists
  (`--kind class,interface`) in addition to repeated flags.
- **CLI** — `extract --format tree` renders a human-readable outline
  built from moniker segments, with refs nested under their source def.
  - `--color auto|always|never` (honors `NO_COLOR`, `CLICOLOR`,
    `CLICOLOR_FORCE`, `TERM=dumb`, `IsTerminal`).
  - `--charset utf8|ascii` for glyph set.
  - Default filter without `--kind` drops `local`/`param`/`comment` defs
    and hides refs — passing any `--kind` re-enables full output.
  - Behind the `pretty` cargo feature (enabled by default). Library
    consumers can opt out with `default-features = false`.
- **`code-moniker-core`** — `Shape::Ref` variant, plus `Shape::ALL`,
  `Shape::for_kind`, and `Shape: FromStr` so the CLI filters via the
  core enum directly.

### Changed

- **CLI** — `--format tree` no longer truncates signatures. The previous
  32-char ellipsis budget hid real type info; pipe through a wrapper if
  narrow output is needed.
- **CLI library** — `ExtractArgs`/`CheckArgs` rename the positional field
  `file` → `path` since the argument accepts directories as well.
- **Docs** — major refresh of the CLI surface in `docs/cli/*`, plus
  `README.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `docs/README.md`,
  `docs/perf.md`. The agent-harness guide now ships end-to-end recipes
  (`.code-moniker.toml` + hook script + `settings.json` + CI command)
  for six common scenarios. The `~600-line file cap` rule is relaxed
  into a preference.

### Removed

- **CLI** — root-level `code-moniker <PATH>` form. Use
  `code-moniker extract <PATH>` instead. All filtering flags moved under
  the verb.

### Fixed

- **Docs** — path encoding table in `docs/cli/check.md` corrected:
  - Go uses `package:<segment>/module:<stem>` (not `dir:`).
  - C# uses `package:<segment>/module:<stem>` (not `dir:`).
  - SQL uses `dir:<segment>/module:<stem>` then nests `schema:<name>`
    under `module:` (the previous `schema:<name>/module:<stem>`
    description had the segments in the wrong order).
- **Docs** — `docs/cli/extract.md` no longer claims the `--format json`
  output is identical to `code_graph_to_spec(graph)`. The CLI emits its
  own `{uri, lang, matches: {defs, refs}}` shape; round-tripping into
  Postgres still goes through `code_graph_declare(jsonb)` with a spec
  payload, not the CLI's match payload.
- **Docs** — `docs/cli/extract.md` ref JSON example now lists the
  `binding` field that the CLI actually emits.
- **Docs** — `docs/cli/extract.md` synopsis lists `--color` and
  `--charset`.
- **CI** — release workflow rewritten for the workspace split: publishes
  `code-moniker-core` then `code-moniker` in dependency order, verifies
  the git tag against the core crate's version. `code-moniker-pg` stays
  excluded from automated publishing for now.

## [0.1.0] — 2026-05-13

Initial public release of the three crates: `code-moniker-core` (pure
Rust foundation), `code-moniker` (standalone CLI + linter), and
`code-moniker-pg` (PostgreSQL extension via pgrx).
