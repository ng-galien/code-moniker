# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The workspace crates share a single version; only `code-moniker-core`
and `code-moniker` are published to crates.io by the release workflow.
Breaking changes are allowed in minor releases as long as the project is
in `0.y.z`.

## [Unreleased]

### Added

- **`code-moniker ui`** — adds a read-only terminal architecture explorer
  over the extracted graph. It reuses the shared cache, shows overview
  metrics, declaration outlines, incoming/outgoing refs, regex name
  filtering, source snippets, and an on-demand `.code-moniker.toml`
  check summary.
- **`code-moniker ui` search mode** — `s` opens a ranked symbol search
  that feeds the contextual navigator, while `/` remains the structural
  regex/kind filter.
- **`code-moniker ui` search input** — active symbol searches now render
  a dedicated search field above `ui.navigator`; the field is visibly
  focused while editing.
- **`code-moniker ui` panel snapshots** — `y` copies a plain-text
  snapshot of the active right panel, including its stable component
  marker, current mode, scope, and panel lines, to the system clipboard
  without blocking the TUI event loop.
- **`code-moniker ui` panel readability** — right-side panels now share a
  structured presentation layer with bold sections, muted hints, reusable
  key/value rows, tabular summaries, and theme-driven panel colors.
- **`code-moniker ui` change mode** — `d` opens a Git-backed
  `HEAD..worktree` view that filters `ui.navigator` to changed
  declarations, marks changed symbols in explorer mode, reports roots
  without Git in single-source and multi-source sessions, and lets `u`
  toggle blast-radius usages without leaving change mode. Added,
  modified, and removed declarations are represented in the change
  navigator. The UI now keeps the store live through filesystem watchers:
  source changes reload the in-memory index, while `.git` changes refresh
  the change index; generated build paths and custom cache directories
  are ignored.
- **`code-moniker extract --format text`** — extract now defaults to a
  moniker-only text output (`txt` alias supported). `--format tsv`
  remains available for metadata columns. Compact monikers use a concise
  form such as `java:app.user/UserService.class:UserService`, text output
  colorizes automatically on TTYs, `-c` forces color, and
  `--moniker-format uri` restores full `code+moniker://` URIs.
- **`code-moniker extract --name <regex>`** — filters defs by their
  last moniker segment name, and refs by target name, before rendering.
  This keeps `--format tree` usable for queries such as Java interfaces
  ending with `Resolver`.
- **`code-moniker stats`** — reports extraction-only metrics for a file
  or directory: supported file counts by language, def/ref totals,
  shape and kind distributions, plus scan/extract/total wall-clock
  timings in milliseconds. Formats: `tsv`, `json`, and colored `tree`.
- **Multi-source inspection** — `code-moniker stats` and
  `code-moniker ui` now accept several source roots in one command
  (for example `code-moniker stats service-a service-b`). Each root is
  labelled and prefixed in the session graph so same-path files from
  different repositories remain distinguishable.
- **`code-moniker manifest` / Cargo** — Cargo workspace manifests now
  expose `workspace_member` rows, workspace dependency rows, and
  path-backed dependency metadata in JSON output, giving ESAC-style
  consumers enough structure to reconstruct workspace modules and path
  edges.
- **`code-moniker harness codex`** — installs a project-local Codex
  `PostToolUse` live architecture harness from a named profile, writing
  `.codex/hooks.json`, a direct `code-moniker check` hook script, and
  a small hook-overhead report template.
- **`code-moniker check --format codex-hook`** — emits Codex
  `PostToolUse` `decision: "block"` JSON for architecture violations,
  carrying the failing rule id and diagnostic instead of a generic hook
  exit-code failure. The Codex harness now uses this format directly.
- **`code-moniker harness claude`** — installs the same live
  architecture harness under project-local `.claude/` files only, with
  Claude Code's `exit 2` mapping for `PostToolUse` repair feedback.
- **`code-moniker-core` (rs extractor)** — Rust test declarations now
  emit `test` defs with stable monikers, source positions, framework
  metadata (`rust-test` / `proptest`), display names, disabled state
  from `#[ignore]`, and inline module hierarchy for ESAC-style test
  taxonomy consumers.

### Changed

- **MSRV** — minimum supported Rust version is now 1.86, matching the
  `ratatui 0.30` requirement used by the new terminal UI.
- **CLI color handling** — `extract --format text`, `extract --format
  tree`, and `stats --format tree` now share one `--color` decision:
  explicit `always` / `never` wins, while `auto` honors terminal
  environment variables.
- **Release versioning** — `main` now tracks the next planned Cargo
  release version (`0.3.0`) instead of retaining the already-tagged
  version. Crates inherit `version.workspace = true`, the internal
  `code-moniker-core` dependency is bumped once in workspace
  dependencies, `code-moniker-pg` is marked `publish = false`, and
  release CI verifies crates.io packages share the tag version.
- **`code-moniker extract --format tree`** — tree output now collapses
  linear filesystem and namespace branches inline, so paths such as
  `src/main/java` and package chains such as `org.apache.bookkeeper`
  render as one IDE-style branch instead of one row per segment.
- **`code-moniker ui`** — the left pane now defaults to a collapsible
  navigator (`language -> directory -> file -> symbol`) instead of a
  global declaration list.
- **`code-moniker ui`** — filtering now narrows the navigator in place
  instead of switching to a flat list. Filters keep ancestor context,
  remain navigable with the same tree keys, and accept `kind:<kind>`
  clauses such as `kind:interface Resolver`.
- **`code-moniker ui`** — navigator rows now compact linear branches
  (`lang -> dir -> file`) into one row and auto-open down to the first
  real branch point, matching the compact tree output style.
- **`code-moniker ui`** — TUI colors are now centralized behind theme
  tokens for navigation, status, sections, and source snippets.
- **`code-moniker ui`** — keyboard handling now follows a small
  Elm-style `Msg -> update -> render` loop with explicit normal and
  filter-editing modes. Index access is isolated behind `ui::store`,
  and the architecture profile now guards that only the store adapter
  imports `SessionIndex`.
- **`code-moniker ui`** — visible component markers such as
  `[ui.navigator]`, `[ui.panel.refs]`, and `[ui.status]` now identify
  stable UI zones for bug reports, feedback, and future feature-module
  contracts.
- **`code-moniker ui`** — introduces the first contract-driven TUI shell
  layer: typed `Route`, `Effect`, `Screen`, and `Feature` contracts, a
  static feature registry, and an `ExplorerFeature` that declares the
  current overview/outline/refs/check navigation surface.
- **`code-moniker ui`** — visualization modes are now explicit UI
  state. The header now carries only global orientation (`mode` and
  `scope`), while contextual panels can follow navigator selection when
  the view is not manually pinned.
- **`code-moniker ui`** — the refs panel is now impact-oriented:
  incoming references are shown before outgoing dependencies, and refs
  sharing the same visual context are grouped into width-aware component
  rows with aggregated kinds, location, confidence, and compact moniker
  details instead of full moniker URIs.
- **`code-moniker ui`** — declaration and reference kinds now use a
  centralized theme palette, grouping callable, type-like, value, module,
  reference, metadata, and fallback colors.
- **`code-moniker ui`** — explorer symbols now sort by each language's
  `KindSpec` order before source position, so types, callables, and
  values appear in a stable semantic order.
- **`code-moniker ui` / Java** — Java value members now sort before
  callables in the explorer, so record component fields stay visible
  before their generated accessors.
- **`code-moniker-core`** — each language extractor now exposes a
  semantic definition-kind contract (`KindSpec`) with shape, display
  label, and ordering metadata. The UI consumes this contract for
  navigability, kind ordering, and color grouping instead of hard-coding
  language-specific kind lists.
- **`code-moniker-core` (rs extractor)** — free function calls now
  resolve against their enclosing Rust module, including explicit nested
  module paths such as `tests::mk_under()` and repeated
  `super::super::...` paths. This resolves local test factories/helpers
  without masking genuinely unresolved project calls.
- **`code-moniker-core` (rs extractor)** — common Rust iterator-chain
  methods on call receivers and built-in macros now carry `external`
  confidence and `external_pkg:std` targets, reducing noisy unresolved
  ESAC gaps while leaving identifier-receiver project calls actionable.

### Fixed

- **`code-moniker-core` (rs extractor)** — Rust `const` and `static`
  items now emit defs matching the language kind contract.
- **`code-moniker-core` (java extractor)** — Java record components now
  emit private field defs, public accessor method defs, and `uses_type`
  refs for their component types. Explicit record accessors are not
  duplicated.
- **`code-moniker-core` (java extractor)** — `this`/`super` method calls
  no longer resolve to a same-name Java overload when the argument arity
  does not match.
- **`code-moniker ui`** — filter entry is now modal: `/` edits a draft,
  `Enter` applies it, `Esc` cancels editing, and normal-mode `x` clears
  the active filter. Normal-mode `Esc` closes navigation nodes instead
  of quitting the UI; use `q` or `Ctrl-C` for explicit exit.
- **`code-moniker ui`** — `Esc`/left now behaves as a back action in
  filtered modes: it closes navigation when possible, otherwise it
  clears an empty or invalid search/usages scope and returns to
  explorer mode.
- **Cache correctness** — graph cache entries are invalidated for this
  release so Java record component fields/accessors are not hidden by
  stale graphs from older extractor semantics.
- **`code-moniker ui`** — source snippets now preserve indentation and
  use a light-theme-friendly editor palette with muted context lines,
  a pale active-line background, and clearer line numbers.
- **Cache correctness** — cache keys now include the extraction context
  (`--project` and TS path aliases), so changing context cannot reuse a
  graph extracted with stale monikers.
- **Multi-source TS aliases** — TypeScript path aliases are rebased into
  their labelled source root, keeping `@/*` imports connected to the
  correct service when inspecting several roots together.
- **`code-moniker ui`** — Rust `fn` declarations are now navigable, and
  filter counters only count declarations that can actually appear in
  the navigator.
- **`code-moniker ui`** — multi-source navigator compaction now stops
  before file and symbol rows, so single-file services remain visible as
  source-root directory rows instead of looking like class-only entries.
- **`code-moniker ui`** — pressing `u` on a selected declaration focuses
  the navigator on usages of that symbol and shows the matching
  references in the refs panel. For multi-source Java inspection, this
  also matches compatible import targets across source roots, which makes
  shared-library consumers visible from the library symbol.

## [0.2.0] — 2026-05-13

### Added

- **`code-moniker check`** — `--report` appends per-rule observability
  counts in text and JSON output. Implication rules include
  `antecedent_matches` and warn when the left-hand side never matched
  the scanned graph, making scan-root-relative architecture aliases
  easier to diagnose.
- **CLI** — `extract --project <NAME>` sets the project component of
  the anchor moniker (default `.`); composes with `--scheme`. Cache
  keyed on the anchor hash, so different projects coexist on disk
  without collision.
- **`code-moniker-core` tests** — snapshot + conformance harness
  (`crates/core/tests/extractor_{snapshots,conformance}.rs`, insta-driven)
  over real-code fixtures in `crates/core/tests/fixtures/<lang>/`. The
  full def/ref graph (every attr, every confidence) is the
  anti-regression surface. Replaces the bulk of inline micro-tests.
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

- **`code-moniker-core` (sql/rs/cs strategies)** — internal perf/clean
  pass: SQL `Strategy.callable_table` borrowed (no clone per
  plpgsql re-parse); Rust `receiver_hint` returns `&[u8]` (was `Vec<u8>`,
  matches other langs); C# `clr_system_path` returns `Option<&'static
  [&'static str]>` (no per-call alloc).
- **`code-moniker-core` (test helpers)** — `assert_conformance` /
  `assert_local_refs_closed` exposed `pub` for integration tests, marked
  `#[doc(hidden)]`.
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

- **`code-moniker-core` tests** — 128 shape-probe micro-tests across
  the seven per-lang `mod tests` blocks (629 → 501 lib tests).
  Subsumed by the new snapshot + conformance fixtures. Kept tests
  target error paths, Presets, `deep=true`, syntax absent from
  fixtures, tree-sitter gotchas, and documented regressions.
- **CLI** — root-level `code-moniker <PATH>` form. Use
  `code-moniker extract <PATH>`; filters moved under the verb.

### Fixed

- **`code-moniker-core` (rs extractor)** — `pub`/`pub(crate)` now emit
  `visibility = public/module`; methods inside `impl Trait for T` inherit
  public via the trait. `impl X for Enum` no longer shadows with a phantom
  `struct:Enum`. `write!()`/`format!()` target `macro:` (was `fn:`).
  `Self {...}` / `Self::new()` resolve to the impl type. `method_call`
  populates `receiver_hint` (self / call / member / identifier text).
- **`code-moniker-core` (ts/python/java extractors)** — `import_targets`
  map routes `uses_type` / `method_call` / `calls` / `instantiates`
  through imported symbols: `z.object()` →
  `external_pkg:zod/path:z/method:object`, `Protocol` →
  `external_pkg:typing/function:Protocol`, `List<T>` →
  `external_pkg:java/path:util/path:List`. (TS) `helper()` and `new
  Widget()` on a `import { … } from "./y"` name now target
  `module:y/function:helper` / `module:y/class:Widget` (was the local
  module); bare re-export `export { X };` after such an import emits
  a `reexports` ref to the import target.
- **`code-moniker-core` (java extractor)** — `java.lang.*` classes
  (`String`, `Exception`, `RuntimeException`, …) resolve implicitly to
  `external_pkg:java/path:lang/path:X`. Primitives skipped from refs.
- **`code-moniker-core` (cs extractor)** — well-known CLR types
  (`Task`, `IAsyncEnumerable`, `ConcurrentDictionary`, `Exception`, …)
  resolve to `external_pkg:System/path:.../path:X` even without an
  explicit `using`.
- **`code-moniker-core` (go extractor)** — `var ErrFoo = …` emits
  visibility (capitalized → public). Built-in primitives
  (`string`/`int`/`error`/…) skipped from `uses_type`.
- **`code-moniker-core` (sql extractor)** — same-file qualified calls
  (`app.make_id(...)`) resolve to the defined `function:make_id(p:text)`
  signature; `callable_table` propagates into `walk_plpgsql_body` so
  plpgsql bodies see outer-file definitions.
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
- **`code-moniker-core` (ts extractor)** — `reads`, `calls`,
  `uses_type`, and `instantiates` to a binding captured from an outer
  frame target the defining frame's def (e.g. `outer({x})/param:x`,
  `outer()/type:Local`, `outer()/class:Local`), not a synthetic
  segment appended to the inner frame's scope or to the module root.
  Scope frames now carry the exact def moniker per binding; type
  aliases, interfaces, enums, and classes declared inside a callable
  register in the same scope.
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
