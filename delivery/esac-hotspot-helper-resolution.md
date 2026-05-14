# ESAC Hotspot Helper Resolution Delivery

## Goal

Reduce ESAC Tier 1 gaps caused by Rust helper/factory calls without weakening
the code-moniker graph model. The delivery must improve provable linkage, not
hide unresolved project calls.

## Scope

- Add focused Rust fixtures/tests for same-module helpers, nested `mod tests`
  factories, and strategy-style helpers such as `cfg_from`, `build_module`,
  `mk`, and `mk_under`.
- Extend Rust callable lookup so calls resolve only when a matching local def
  exists in the current module, an enclosing module, or an explicit nested
  module path.
- Keep unresolved project-like calls unresolved when no local callable can be
  proven.
- Avoid blanket externalization of short names (`mk`, `child`, `default`,
  `helper`, etc.).

## Non-Goals

- No generic “anti-gap” preset.
- No silent reclassification of arbitrary short identifiers as `external`.
- No ESAC-only post-processing outside extractor semantics.

## Validation

- TDD first: failing extractor tests for helper/factory resolution.
- Existing snapshot/conformance tests must remain green.
- Run the CLI sanity suite and architecture harness.
- Use ESAC after the code change to compare the listed hotspot files before
  and after; real project misses must remain visible.

## Expected Result

ESAC hotspot counts should shrink for local helper/factory calls, while
genuinely unknown calls keep `unresolved` / `name_match` confidence and remain
actionable.

## Local Before / After

Baseline from ESAC before the patch:

| File | Tier 1 gaps |
| ---- | ----------- |
| `crates/cli/src/check/eval.rs` | 203 |
| `crates/core/src/lang/ts/strategy.rs` | 131 |
| `crates/core/src/lang/rs/strategy.rs` | 107 |
| `crates/core/src/lang/java/strategy.rs` | 85 |
| `crates/core/src/core/code_graph/mod.rs` | 84 |

Local extractor comparison using the previously installed binary versus the
patched workspace:

| File | unresolved `calls` before | unresolved `calls` after |
| ---- | ------------------------- | ------------------------ |
| `crates/cli/src/check/eval.rs` | 203 | 101 |
| `crates/core/src/lang/rs/strategy.rs` | 102 | 102 |

The shrink in `eval.rs` comes from local nested test helpers such as `child`,
`cfg_from`, `build_module`, `submodule`, and `build_root`. Non-local names such
as `from_utf8`, `with_capacity`, and `default` remain unresolved, preserving the
actionable model boundary for future explicit external presets.
