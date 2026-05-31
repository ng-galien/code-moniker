# CLAUDE.md

This file is methodology only. It carries no project facts — no crate map, no
module layout, no language or grammar gotchas, no snapshots of past sessions.
Those rot. The project's knowledge does not live in prose, and it does not live
here.

## Where the project's knowledge lives

The knowledge of this project lives in its **rules** (`.code-moniker.toml`), its
**fragments** (`code-moniker.fragment.toml`), and the **rationales** attached to
them. That is the authority on what each module owns, forbids, and why.

- **Discover before you act.** Before changing code you do not already
  understand, find the rules, fragments, and rationales that constrain it. The
  MCP server is the first tool for this: `code_moniker_read workspace` for the
  shape, `code_moniker_read workspace/views` then a view for boundaries and their
  rationale, `code_moniker_symbols` to locate structure, `code_moniker_rules` for
  the active guardrails on the file you are about to touch. Failing the MCP, read
  the nearest `code-moniker.fragment.toml` and the root `.code-moniker.toml`.
- **Build to the rationale, not to the `expr`.** The rationale is the design
  contract. Satisfying a rule's expression while violating its intent is a
  failure, not a pass.
- **Encode what you learn where the tooling can enforce or surface it.** A
  durable finding becomes a rule, a fragment, a rationale, a view, a
  `docs/cli/check-samples/` sample, an `evolutions/` entry, or a focused test —
  never prose in this file. A gotcha in a comment or a markdown page is knowledge
  waiting to rot; a rule fails the build when violated, a view surfaces through
  the MCP.

## Tests

Test policy is the one substantive exception to the minimalism above, because it
is not yet formalized as rules. It is spelled out in full here.

### What a test asserts

- **Test the contract, not the structure.** Tests exercise a module's durable
  contract through its observable outputs — returned values, diagnostics,
  persisted effects, emitted records. Tests that lock the current internal
  structure are disallowed when they compete with that contract. Inline
  `#[cfg(test)] mod tests` is acceptable only for a named sub-component whose
  local contract is itself stable and intentionally exposed.
- **Snapshot the public model only.** Never snapshot mutable state, wiring,
  helper types, cursors, transition objects, or other accidental implementation
  detail.
- **Keep robustness coverage separate** and express it as property or fuzz tests
  over invariants: does not panic, terminates, rejects without corrupting state,
  preserves a round-trip, stays within a bound, holds a declared consistency
  property.

### Where tests live

- CLI behavior in `crates/cli/tests/`.
- SQL surface in numbered `pgtap/sql/*.sql`, run through `./pgtap/run.sh`.
- Extractor changes with focused fixtures under `crates/core/tests/fixtures/`,
  with intentional proptest regression updates.

### How to run them

- **Narrowest-first.** Run the focused test for the change; widen the gate only
  once the behavior is stable. Do not rerun the full workspace suite after every
  small edit. Run Cargo validations sequentially — parallel `check`/`test`/
  `clippy` contend on the same target directory.
- **Iteration loops:**
  - UI: `cargo test -p code-moniker ui::tests::<test_name> --lib`, then
    `cargo test -p code-moniker ui::tests --lib` once the flow works.
  - CLI: `cargo test -p code-moniker --test cli_e2e <test_name>`.
  - Extractor: the focused language unit test, then
    `cargo test -p code-moniker-core snapshot_<lang>` or its conformance test.
  - Guardrail: `cargo moniker-check` only when touching module boundaries, rules,
    imports, or module organization.
- **Pre-review gate:** `cargo fmt --all -- --check`,
  `cargo check --workspace --exclude code-moniker-pg --all-targets`, the focused
  test group, and `cargo moniker-check` when boundaries are involved.
- **Pre-commit gate** (non-release, once after review fixes):
  `cargo test --workspace --exclude code-moniker-pg --quiet`,
  `cargo clippy --features pg17 --no-default-features --tests --no-deps -- -D warnings`,
  and `cargo test --features pg17 --no-default-features --lib`. Run
  `./pgtap/run.sh` only when `crates/pg`, SQL types, or extension behavior
  changed.
