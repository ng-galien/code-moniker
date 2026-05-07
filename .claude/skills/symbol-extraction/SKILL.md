---
name: symbol-extraction
description: Reference and workflow for writing per-language symbol extractors in pg_code_moniker (`src/lang/<lang>/`). Use this skill for any task that adds, extends, ports, or debugs an extractor for TypeScript/JavaScript/TSX/JSX, Java, Python, SQL/PL/pgSQL, Rust, Go, C#, C/C++, or PHP — including emitting defs and refs into `code_graph`, computing typed `+moniker` identities from AST nodes, picking kinds, handling deep extraction (parameters, locals, callbacks), wiring tree-sitter grammars, fixing canonicalization bugs, or interpreting `docs/EXTRACTION_TARGETS.md` / `docs/MONIKER_URI.md`. Trigger on phrases like "ajoute un extracteur", "extract symbols", "moniker pour ...", "ref kind", "deep extraction", "canonicalize", "anchor moniker", "tree-sitter walker", or whenever code under `src/lang/` is touched.
---

# Symbol extraction — pg_code_moniker

Authoritative reference for writing and reviewing language extractors. The
target documents are:

- `docs/EXTRACTION_TARGETS.md` — extraction parity bar for ESAC.
- `docs/MONIKER_URI.md` — canonical `scheme+moniker://` identity format.

This skill condenses the contract, file layout, and per-language targets so
work on any extractor stays consistent.

The extension is stateless. An extractor is a pure function:

```rust
extract(uri, source, anchor: &Moniker, presets: &ExtractPresets) -> CodeGraph
```

No table reads. No order-dependence. No backend-local ids in produced moniker
bytes. Same input → same `CodeGraph`, byte-for-byte. Current code may still
carry transitional helpers such as `KindRegistry`; do not treat them as the
target design for persisted identity.

## What an extractor must produce

A `CodeGraph` rooted at the module's own moniker (computed from `anchor` plus
URI-derived path segments), then defs and refs that satisfy the **common
contract** below. Every extractor across every language emits the same row
families, only the AST shapes differ.

### Defs — required fields

For each definition node:

- `moniker` — built by extending the parent's typed segment chain. Canonical
  display is `scheme+moniker://.../kind:name#kind:name`; bytes must be stable
  for the same source and must not depend on backend-local ids.
- `kind` — semantic label kind (`class`, `interface`, `function`, `method`,
  `field`, `const`, `local`, `param`, …). Use the shared vocabulary from
  `EXTRACTION_TARGETS.md` § Common Contract and `references/kinds.md`.
- `parent` — the enclosing def's moniker. Always already in the graph at the
  point of insertion (walker emits parents before children).
- `position` — `(start_byte, end_byte)` from the AST node. `None` only for
  synthetic/external graphs that have no source.

Visibility, signature, and type metadata are required by the extraction
contract even when the transitional `DefRecord` cannot store them yet. Until
`DefRecord` is extended, encode overload identity into the method/function
moniker and leave an explicit TODO pointing at the language reference for the
missing metadata.

### Refs — required fields

For each reference:

- `source` — innermost enclosing def. Top-level refs anchor on the module
  root; intra-class refs anchor on the class (or method) where the ref lives.
- `target` — full target moniker when locally determinable; otherwise an
  explicit unresolved/external moniker shape. Never a partial moniker.
- `kind` — shared vocabulary: `calls`, `method_call`, `reads`, `instantiates`,
  `extends`, `implements`, `uses_type`, `imports_module`, `imports_symbol`,
  `reexports`, `annotates`, `di_register`. Pick the closest match; do not
  invent kinds without updating `EXTRACTION_TARGETS.md`.
- `position` — byte range of the ref in source.

Unresolved/name-only refs are legitimate but must be **explicit** — not
silently encoded as a partial moniker.

### Deep/resource-scoped defs

Parameters, locals and callbacks are **in scope**. They may be excluded from
ESAC's repo-wide symbol projection, but they must be extractable from the
resource-scoped graph for outline, local reasoning, coverage attribution and
LLM planning. Do not treat locals as out-of-scope just because they are not
global linkage targets.

### Determinism

For the same `(uri, source, anchor, presets, extractor_version)`:

- no AST traversal that depends on `HashMap` iteration order;
- no `std::collections::HashSet` for ordered output;
- duplicates are deterministic (we accept duplicates in `ref_targets`, but
  they must arise in the same order each run);
- overload arity is computed from the static signature, never from a counter
  that depends on visit order across files.

## File layout for a new language

The TypeScript extractor (`src/lang/ts/`) is the canonical layout. Replicate
it; do not reorganize.

```
src/lang/<lang>/
  mod.rs            pub fn parse, pub fn extract
  walker.rs         AST traversal + def emitters
  canonicalize.rs   moniker construction (compute_module_moniker, extend_*)
  refs.rs           refs extraction (impl Walker for ref-emitting nodes)
  kinds.rs          per-language Kinds struct: canonical structural kinds
                    + semantic labels
```

Each file under ~600 lines. When a file approaches the cap, split the
production module — never extract the tests. Tests live in `#[cfg(test)] mod
tests` next to the production code, per the project's TDD convention.

A new language also requires:

1. `Cargo.toml` — add the `tree-sitter-<lang>` grammar crate.
2. `src/lang/mod.rs` — `pub mod <lang>;`.
3. `src/pg/extract.rs` — a thin `#[pg_extern] extract_<lang>(...)` SQL entry.
4. `tests/fixtures/<lang>/` — source fixtures with snapshotted `code_graph`
   expectations (Rust constants), once the extractor is past the
   one-test-per-shape phase.

## Workflow — the order to add or extend an extractor

This is the loop. Skipping steps in the name of speed produces extractors that
are hard to debug at fixture scale.

1. **Read the target.** Open `docs/EXTRACTION_TARGETS.md` § Language Targets
   for the language. Note required def kinds, required ref kinds, deep-extr
   requirements, and explicit non-targets (e.g. "no whole-program Java
   resolution"). Anything outside the language section's bullets is a feature
   request, not a bug.
2. **Pick the next failing case.** TDD cycle. Write one test in `mod tests`
   that names the invariant (`extract_<shape>_emits_<expected>`), with a
   minimal source fixture. Confirm it fails for the right reason.
3. **Wire the kind, then the AST shape.** Add the semantic kind to
   `kinds.rs` / the language kind table. Add the AST node match in
   `walker.rs` and the corresponding emitter.
4. **Canonicalize once, in `canonicalize.rs`.** Never inline moniker
   construction in `walker.rs` or `refs.rs` — moniker shape is a contract,
   centralize it.
5. **Run pure-Rust tests first.** `cargo test --lib lang::<lang>` is the fast
   loop (sub-second). It exercises the extractor without booting Postgres.
6. **Run the SQL surface only when needed.** Use the `pgrx-reinstall` skill
   (build + install + pgTAP) to validate `extract_<lang>` end-to-end after a
   batch of pure-Rust passes. SQL is tested in SQL via `test/sql/*.sql`.

Extractors are introduced one at a time. Do not split attention across
languages within a single change.

## Decision points — when to consult a reference file

The per-language pages are progressive disclosure. Read the one(s) you need;
skip the rest.

- TypeScript / JavaScript / TSX / JSX → `references/typescript.md`
- Java → `references/java.md`
- Python → `references/python.md`
- SQL / PL/pgSQL → `references/sql.md`
- Rust, Go, C#, C/C++, PHP → `references/next_wave.md`

Cross-cutting concerns each reference file refers back to:

- moniker construction patterns → `references/canonicalization.md`
- kind vocabulary and URI discipline → `references/kinds.md`
- deep extraction (parameters, locals, callbacks) → `references/deep.md`

## Compass — the ESAC test

Every extracted def or ref must serve at least one of:

- `esac_symbol find` / `refs` / `carriers` / `families` / `health` / `gaps`
- `esac_outline`
- resource-scoped analysis over a single `code_graph` (locals, params,
  callbacks, local impact, planning before write)

If a feature does not feed one of those operations, it does not belong in the
extractor. This is the same compass `CLAUDE.md` applies to the whole
extension — extractors inherit it.

## What not to do

- **Do not invent ref kinds.** The vocabulary is closed; extending it is a
  spec change, not an extractor change.
- **Do not produce partial monikers.** A target moniker is either fully
  determinable from local source or it is an explicit unresolved/external
  shape. There is no third option.
- **Do not read tables, lookup other modules, or call PG functions.** The
  extension is stateless; cross-module resolution happens in SQL via the
  moniker `=` operator on the consumer side.
- **Do not skip the `position` field on extracted graphs.** Source-backed
  defs and refs always carry byte ranges. `None` is reserved for synthetic /
  external graphs.
- **Do not accept "good enough" determinism.** If a test case is flaky
  across runs, fix the iteration order before merging — at corpus scale,
  non-determinism becomes unfindable diffs.
