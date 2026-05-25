# Workspace Session Bridge Review

Status: WIP review findings for `workspace-semantic-ports`.

The current branch introduces a parallel `workspace::session` model, local
resource implementations, and a `SessionStoreBridge` anti-corruption layer that
adapts the new model back to the legacy `IndexStore` surface.

The bridge is intentionally not considered complete until the following review
findings are resolved or consciously re-scoped.

## Findings

- High: the bridge is not actually wired into the UI runtime. `App`, `AppStore`,
  and async tasks still own and construct concrete `WorkspaceStore` values, so
  `SessionStoreBridge` cannot replace the runtime without a store-handle
  refactor.

- High: `check_summary` does not preserve the legacy contract. The new session
  load currently collects diagnostics eagerly, so invalid rule configuration can
  fail workspace loading instead of failing only the check action. The bridge
  `check_summary` also ignores its `rules`, `profile`, and `scheme` parameters.

- High: `LocalLinkage` is not semantically equivalent to the legacy linkage
  index. It currently resolves by simple local matching and does not model
  external references, manifest-blocked references, ambiguous matches,
  projectless/callable matching, or the legacy `incoming_refs_for_def` behavior.

- High: git/change parity is incomplete for real repositories. File change
  counts currently rely on parsing `SourceId`, and removed entries without a
  symbol location lose language and file path information. Existing tests mostly
  cover the no-change case.

- Medium: local indexing can silently substitute an empty source string when a
  source read fails after extraction. The legacy index reports that failure.

- Medium: custom URI schemes are not carried consistently. The local code index
  records identities with `DEFAULT_SCHEME`, while diagnostics can evaluate with
  a caller-provided scheme.

## Missing Coverage

- UI/runtime integration with the bridge.
- Invalid rules during workspace load versus check execution.
- Real git changes, including modified and removed symbols.
- Manifest-blocked, ambiguous, external, and projectless linkage.
- Custom URI scheme mapping between index records and diagnostics.
- Source read failures after extraction.
