# Workspace Session Bridge Review

Status: current migration baseline established on `workspace-semantic-ports`.
The target model now has explicit `source`, `code`, `linkage`, `changes`,
`snapshot`, `compat`, and `legacy` boundaries. The bridge remains an
anti-corruption adapter over the legacy `IndexStore` surface.

The bridge is intentionally frozen until the final swap. Do not keep polishing
the bridge as if it were the target architecture. It may carry compatibility
debt, legacy `DefLocation` conversion, and UI/runtime adapter concerns. New
design work should happen in the target boundaries and be exposed through
stable read models; the bridge should only be adjusted when required to keep the
current UI/runtime functional.

This document tracks what the baseline actually covers and what remains before
the target model can replace the legacy runtime.

## Resolution Summary

- Runtime bridge: intentionally deferred. `WorkspaceHandle` can load the
  session-backed bridge, but the UI still consumes the legacy `IndexStore`
  surface through a compatibility facade. This does not move until the final
  swap.
- Check coupling: resolved by moving workspace-aware rule execution to
  `check::workspace`; workspace refresh no longer builds rules.
- Linkage model: baseline established with local/global resolution,
  language-strategy delegation, external classification, manifest policy,
  ambiguity accounting, and report projection from a decision log. Java
  acceptance is covered by a versioned multiproject fixture with `0`
  unresolved references.
- Change model: resolved for this slice with `ChangeAnalyzer` consuming
  `SymbolProvider`, `SymbolLocation` replacing legacy `DefLocation`, and
  removed-entry metadata preservation. Full real-repository modified/removed
  acceptance remains open.
- Source identity: resolved for known workspace paths through
  `SourceCatalogMaterial` canonical URI lookup; lazy rule-derived resources are
  represented as the next extension point on the same boundary.
- Source read failures: resolved by explicit code-index failures and tests.
- Custom URI schemes: resolved through shared identity configuration and tests.

## Completion State

| Priority | Area | Work |
| --- | --- | --- |
| Done | Lazy check resources | Source catalog options can index an eager subset while source identity/content/symbol-provider boundaries resolve and load a rule-derived source outside the eager index. Covered by `symbol_provider_loads_rule_derived_source_outside_eager_index`. |
| Done | SymbolProvider boundary | `SymbolProvider` can answer for eager indexed symbols and for path-derived lazy symbols. The lazy path is intentionally covered through a boundary fixture, not internal extractor tests. |
| Done | Source URI centrality | `SourceCatalogMaterial` resolves known and root-contained paths to canonical source resources. Consumers ask the source boundary instead of constructing source URIs directly. |
| Done | Linkage acceptance | Focused target tests cover manifest-blocked, ambiguous, and projectless Java linkage decisions without using the legacy linker as oracle. |
| Done | Change acceptance | A real git fixture covers modified and removed symbols, including a removed symbol with no current workspace symbol. |
| Done | Workspace rules | The active guardrail reports no target-boundary errors. Remaining warnings are in frozen `legacy`/`compat` bridge debt. |
| Deferred | Bridge/UI runtime | Leave the bridge/UI runtime surface stable until the final swap. Only touch it to preserve behavior while target boundaries evolve. |
| Assumed debt | Java extractor | The Java `Strategy` remains a large extractor module. This debt is accepted for now; do not expand tests around its internals. Cover behavior through fixtures and acceptance. |

## Findings

- High: the bridge is not the target runtime model.

  Current status: deferred until the final swap. `WorkspaceHandle` can carry a
  session-backed bridge, and UI code can consume the handle. That is enough for
  the migration slice. The bridge remains a compatibility facade over the
  legacy `IndexStore` contract and may keep `DefLocation` conversions.

  Decision: do not refactor the bridge further during target-model work. Build
  the target boundaries beside legacy, keep contract tests green, and swap the
  runtime in one final gesture once the target model is complete.

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

- High: `LocalLinkage` is not yet fully covered by target acceptance tests for
  every linkage decision class.

  Decision: accepted. The correction is Option A: extract a proper linkage
  model rather than treating `LocalLinkage` as a placeholder. Linkage must be
  split between local linkage, which can be decided inside one source file, and
  global linkage, which resolves across sources/projects/manifests. The model
  must delegate language-specific linking semantics to per-language strategies.
  Linkage is expected to evolve frequently, so it needs a clean, low-adhesion
  contract; callers should consume stable linkage outputs instead of depending
  on the current implementation shape.

  Current status: baseline implemented. The resolver produces a
  `LinkageDecisionLog`, and `LinkageGraphReport` is a projection of decisions.
  Java linkage is exercised through
  `crates/cli/tests/fixtures/workspace/java/multiprojet`, including local,
  global, runtime external, field, callable arity, source-set, and `var`
  inference cases. Remaining tests must target manifest-blocked, ambiguous, and
  projectless decisions directly.

- High: git/change parity is incomplete for real repositories.

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

## Coverage State

Covered:

- Workspace refresh does not compile or own check rules.
- Check execution lives in `check::workspace` and consumes the workspace model.
- Source read failures fail code indexing explicitly.
- Custom source/symbol identity schemes are covered for local indexing and
  diagnostics.
- Known workspace paths can be resolved to canonical source URIs.
- Removed changes without current symbols preserve observable metadata.
- Java multiproject linkage is covered by a versioned fixture with zero
  unresolved references.

Missing:

- Final runtime swap away from the bridge facade.
- DSL-level syntax/operators for lazy check obligations. The underlying source
  identity/content/symbol-provider path exists, but the check language still
  needs the rule-facing expression surface.

Decision: accepted as completion criteria for the target model. The bridge must
not be considered runtime-replaceable until the target model is swapped into the
runtime. Do not add new legacy parity tests as a substitute for target
contracts.

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

Current status: boundary implemented and covered without changing the bridge
contract. The remaining work is DSL-facing: expose this capability to check
rules without letting the workspace own rule construction or letting consumers
assemble URIs by hand.
