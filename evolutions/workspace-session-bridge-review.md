# Workspace Session Bridge Review

Status: review findings resolved for the current `workspace-semantic-ports`
migration slice. Remaining warnings are general smell-rule candidates, not
workspace-boundary violations.

The current branch introduces a parallel `workspace::session` model, local
resource implementations, and a `SessionStoreBridge` anti-corruption layer that
adapts the new model back to the legacy `IndexStore` surface.

The bridge is considered complete for this migration slice when evaluated
against the findings below: workspace loading does not own check rules, the
target model uses source/code/linkage/changes/snapshot boundaries, compatibility
is isolated, `DefLocation` no longer leaks into target code/change modules,
and the target model has contract tests for the previously missing cases.

## Resolution Summary

- Runtime bridge: resolved through the `WorkspaceHandle`/bridge path and UI
  runtime tests.
- Check coupling: resolved by moving workspace-aware rule execution to
  `check::workspace`; workspace refresh no longer builds rules.
- Linkage model: resolved for this slice with local/global resolution,
  projectless matching, external classification, manifest-blocked accounting,
  ambiguous reference accounting, and language-strategy delegation.
- Change model: resolved for this slice with `ChangeAnalyzer` consuming
  `SymbolProvider`, `SymbolLocation` replacing legacy `DefLocation`, and
  modified/removed git symbol coverage.
- Source identity: resolved for known workspace paths through
  `SourceCatalogMaterial` canonical URI lookup; lazy rule-derived resources are
  represented as the next extension point on the same boundary.
- Source read failures: resolved by explicit code-index failures and tests.
- Custom URI schemes: resolved through shared identity configuration and tests.

## Findings

- High: the bridge is not actually wired into the UI runtime. `App`, `AppStore`,
  and async tasks still own and construct concrete `WorkspaceStore` values, so
  `SessionStoreBridge` cannot replace the runtime without a store-handle
  refactor.

- High: `check_summary` does not preserve the legacy contract. The new session
  load currently collects diagnostics eagerly, so invalid rule configuration can
  fail workspace loading instead of failing only the check action. The bridge
  `check_summary` also ignores its `rules`, `profile`, and `scheme` parameters.

  Decision: accepted as a design-boundary violation. The workspace must not
  build, compile, or own check rules. Rule construction belongs to the check
  action. `WorkspaceSession::refresh` should build sources, code index, linkage,
  and workspace overlays only. `check_summary(rules, profile, scheme)` should be
  the boundary that loads/compiles rules and applies them to a stable workspace
  snapshot. `RuleDiagnosticsPort` should leave the base workspace refresh path;
  diagnostics may be produced by a separate check runner, not by workspace load.

- High: `LocalLinkage` is not semantically equivalent to the legacy linkage
  index. It currently resolves by simple local matching and does not model
  external references, manifest-blocked references, ambiguous matches,
  projectless/callable matching, or the legacy `incoming_refs_for_def` behavior.

  Decision: accepted. The correction is Option A: extract a proper linkage
  model rather than treating `LocalLinkage` as a placeholder. Linkage must be
  split between local linkage, which can be decided inside one source file, and
  global linkage, which resolves across sources/projects/manifests. The model
  must delegate language-specific linking semantics to per-language strategies.
  Linkage is expected to evolve frequently, so it needs a clean, low-adhesion
  contract; callers should consume stable linkage outputs instead of depending
  on the current implementation shape.

- High: git/change parity is incomplete for real repositories. File change
  counts currently rely on parsing `SourceId`, and removed entries without a
  symbol location lose language and file path information. Existing tests mostly
  cover the no-change case.

  Decision: accepted as a model-boundary issue. `ChangeRecord` is the result,
  not the owner of symbol extraction. A change service, tentatively
  `ChangeAnalyzer`, should transform diff regions plus normalized symbols into
  `ChangeRecord` values. `SymbolProvider` is the boundary that provides
  normalized symbols for a source. `ChangeAnalyzer` may ask the `SymbolProvider`
  for symbols of workspace sources, diff-known resources, or diff materialized
  content identified by URI/path. It must not perform ad hoc symbol extraction.
  Change records must keep observable change data even when no current
  workspace symbol can be attached.

  Decision: source URI resolution must be centralized. Consumers must not build
  file/source URIs by hand from absolute or relative paths. Any component should
  be able to ask a dedicated source identity/URI resolver for the canonical URI
  of a physical or logical source, including workspace files and resources
  discovered through diffs. This resolver is a workspace boundary consumed by
  `SourceCatalog`, `CodeIndex`, `SymbolProvider`, `ChangeAnalyzer`, and
  potentially linkage.

- Medium: local indexing can silently substitute an empty source string when a
  source read fails after extraction. The legacy index reports that failure.

  Decision: accepted. Indexing must not silently replace unreadable source
  content with an empty string. A source read failure should either produce an
  explicit `WorkspaceFailure` for the code-index resource or be represented as a
  first-class unavailable-content state with a reason. A plain empty string is
  unsafe because it looks like valid source text and can corrupt snippets,
  positions, diagnostics, linkage, or change localization.

- Medium: custom URI schemes are not carried consistently. The local code index
  records identities with `DEFAULT_SCHEME`, while diagnostics can evaluate with
  a caller-provided scheme.

  Decision: accepted. URI scheme handling belongs to the shared source/symbol
  identity context, not to individual adapters. `CodeIndex`, `SymbolProvider`,
  `ChangeAnalyzer`, linkage, and the check runner must use the same identity
  configuration. Local adapters must not hard-code `DEFAULT_SCHEME` when
  constructing persisted or comparable identities.

## Missing Coverage

- UI/runtime integration with the bridge.
- Invalid rules during workspace load versus check execution.
- Real git changes, including modified and removed symbols.
- Manifest-blocked, ambiguous, external, and projectless linkage.
- Custom URI scheme mapping between index records and diagnostics.
- Source read failures after extraction.

Decision: accepted as completion criteria. The bridge must not be considered
runtime-replaceable until the critical cases above are covered by either legacy
parity tests or isolated contract tests for the new model boundaries.

## Concrete Design Stress Test: Lazy Check Resource

Use case: a check rule is evaluated on an implementation symbol and creates an
obligation on a resource that may not be part of the eagerly indexed workspace.

Concrete example: for an implementation class `Foo`, the check must verify that
a corresponding test class exists before accepting the implementation. The rule
does not simply scan the already loaded index. It derives the expected test
resource URI, asks whether that resource exists, and if needed asks for the
symbols of that resource lazily.

Expected flow:

- The check runner evaluates a rule on an implementation symbol.
- The rule derives or requests the expected test source URI, for example from
  `src/main/java/acme/Foo.java` to `src/test/java/acme/FooTest.java`.
- A source URI resolver returns the canonical URI for that expected resource.
- A source locator/catalog answers whether the resource exists.
- If it exists, a source/content provider loads the resource lazily.
- A `SymbolProvider` returns normalized symbols for that resource.
- The check verifies that the expected symbol exists, for example a class named
  `FooTest`.
- If the URI is missing, or the URI exists but the symbol does not match, the
  check emits a diagnostic.

Design consequences:

- The check is not limited to the eager workspace index.
- The check still must not build or own the workspace.
- The workspace does not build rules.
- Source URI construction must remain centralized; the rule/check asks for an
  expected URI instead of assembling path strings ad hoc.
- `SymbolProvider` must support both eagerly indexed workspace sources and
  lazy, diff-known, or rule-derived resources.
- `CheckRunner` consumes workspace snapshots, source identity, source lookup,
  source content, and symbol-provider ports.
- This use case reinforces the decision to keep `RuleDiagnosticsPort` out of
  `WorkspaceSession::refresh`.
