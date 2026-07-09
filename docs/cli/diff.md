# `code-moniker diff` — symbol-level change facts

`code-moniker diff [PATH]` reports the git changes of a workspace as
**symbol-level facts** instead of line-by-line hunks. The scope is
`HEAD..worktree`.

The output carries facts only — no importance judgment. Business-level
qualification (importance filtering, review auto-acceptance) is planned as a
rules-engine evolution on top of this stable fact model (see
`evolutions/rules-on-change.md`).

## Fact taxonomy

Per-symbol facts (`symbol_changes`), each with a `confidence`
(`certain` — v1 emits only exact-fingerprint pairings):

| kind | meaning |
|---|---|
| `added` / `removed` | symbol only exists on one side (subtrees collapse to their root) |
| `body-modified` | same identity, the body text changed (whitespace-insensitive, own name masked) |
| `signature-changed` | same parent/kind/name, parameter list changed (params are part of the identity) |
| `renamed` | same parent/kind and byte-identical body up to the name, unique 1:1 |
| `moved` | same symbol in another file or container (git rename pairing or exact full-body match) |
| `attribute-changed` | only the header changed (visibility, modifiers) |

`facets` record combinations (`body_changed`, `signature_changed`,
`visibility_changed`, `header_changed`, `file_moved`). Ambiguous pairings
(duplicate bodies, 2:2 destinations) are refused and stay `removed`+`added`.

Reference facts (`ref_changes`): `import-retargeted` and
`call-site-retargeted` pair removed/added references that only differ by a
target renamed or moved in this same diff; the rest stay `ref-added` /
`ref-removed`.

Per-file facts (`files`): disposition (`added`, `removed`, `modified`,
`moved`, `moved-and-modified`), `analyzable` (manifests and unsupported
languages are listed, never silently dropped) and **hunk coverage**: any
changed line not attributed to a reported fact surfaces as `residual`
(comment edits and pure reformatting are residual by design — no fact claims
them).

## Formats

- text (default): grouped per file, pure moves collapsed
  (`= N symbol(s) moved, no other facts`), retargeted references summarized
  (detail with `--refs`).
- `--format json`: versioned envelope `code-moniker.diff/1` with `summary`,
  `files[]` (+coverage), `symbol_changes[]` (identities as moniker URIs),
  `ref_changes[]`, `diagnostics[]`.
