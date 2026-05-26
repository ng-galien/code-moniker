# Rule DSL: Correlated Existence Need

## Need

Some architecture rules need to derive an expected resource from an existing
symbol, then assert that the derived resource exists.

This is not a rule about a specific syntax feature such as Rust enums, and it
should not become a bespoke DSL operator for each new case. The reusable need is
correlated existence:

```text
source symbol -> expected resource or symbol
```

The source symbol exists in the currently evaluated scope. The expected target
may live outside that local file and may require the workspace to resolve or
index a broader scope lazily.

## Concrete Use Case

In the CLI crate:

- `args::Command` is the command registry.
- Each command variant should have an owning module at
  `crates/cli/src/<command>/mod.rs`.
- The module name is derived from the command variant name, for example
  `Extract -> extract` and `Stats -> stats`.

Expected behavior:

- adding `Foo(FooArgs)` to `args::Command` without `src/foo/mod.rs` fails;
- adding `src/foo.rs` instead of `src/foo/mod.rs` fails;
- adding `Foo(FooArgs)` and `src/foo/mod.rs` passes;
- deleting `src/foo/mod.rs` without touching `args.rs` fails in a repo-scoped
  check.

A complete "one command, one module" correspondence also needs the reverse
direction: detecting a command module that has no corresponding enum variant.
That is a separate correlation rule. The `require(...)` primitive below covers
the source-driven direction: a known symbol derives an expected address.

The rule must be expressible without duplicating the command list in TOML.

## Required Building Blocks

The rule needs stable source symbols. Enum-like members should be extracted as
normal child defs where the language has such a concept. The shared kind is
`enum_constant` where possible.

The rule also needs a way to describe an expected target from a source symbol.
For this use case, the expected target is an address with a placeholder derived
from the current enum member name.

The DSL should keep the same expression style as existing predicates and use a
generic requirement predicate:

```toml
require("**/dir:crates/dir:cli/dir:src/dir:{name.snake}/module:mod")
```

`require(...)` accepts an address or URI pattern. The pattern is evaluated in
the current item context. Placeholders such as `{name.snake}` are bound from the
current source symbol, not from a global candidate item.

This is the important abstraction: a local symbol produces an expected address,
and the workspace resolves that address. The rule should not search arbitrary
global bags of definitions, and it should not expose helper functions such as
`module_dir(candidate.moniker)` in the DSL.

Applied to the CLI command enum, the rule would be shaped like:

```toml
[[rust.enum.where]]
id = "command-variants-have-command-modules"
severity = "error"
expr = """
uri ~ '**/module:args/enum:Command'
=> all(enum_constant,
  require("**/dir:crates/dir:cli/dir:src/dir:{name.snake}/module:mod")
)
"""
message = "Each CLI command variant must have an owning module."
```

The public DSL should prefer `uri` over `moniker`. `uri` names the stable
address exposed to rule authors; `moniker` is the internal representation.
Existing `moniker` predicates may remain as compatibility aliases, but new
rules and documentation should use `uri`.

## Scope Requirements

The runtime must keep three scopes distinct.

Activation scope decides which fragments or rules run. In an agent hook, this
is intentionally file-scoped: only fragments on the changed file path should be
considered.

Resolution scope decides where a target is looked up. A rule activated by
`args.rs` may need to resolve `crates/cli/src/<command>/mod.rs`, which is outside
the local source file.

Index scope decides what the workspace loads. If a rule needs a target outside
the local file, the workspace should resolve the minimal required scope lazily.
The check runner should not silently turn every file-scoped invocation into a
full project scan.

## Hook Versus Repo-Scoped Checks

In hook mode, correlated existence is directional guidance:

```text
write crates/cli/src/args.rs
  -> activate the CLI fragment on the args.rs path
  -> evaluate args::Command
  -> derive expected command module addresses
  -> lazily resolve those addresses
  -> report missing modules
```

This catches the common agent mistake when it happens: adding a command variant
without creating the owning command module.

It is acceptable that hook mode is not globally exhaustive. For example,
deleting `src/foo/mod.rs` without touching `args.rs` may not activate the
source-driven rule.

Repo-scoped checks are the completeness gate:

```text
check crates/cli
  -> evaluate the CLI fragment over the crate
  -> resolve the required project targets
  -> report missing or extra command modules
```

## Acceptance Criteria

The final design should support two smoke tests for the CLI command layout.

Local/file-scoped smoke:

- modify `args.rs` by adding a new command variant;
- run a file-scoped check;
- the check reports the missing derived module.

Repo-scoped smoke:

- rename an existing command module directory, for example `stats -> statz`;
- run a repo-scoped check;
- the check reports the mismatch.

The implementation should not expose hardcoded transforms such as
`module_dir(candidate.moniker)` in rule expressions, and it should not introduce
an implicit global side effect into ordinary file-scoped checks.

## Deliverable

The deliverable is working code, not only a design note. It should include:

- the extractor support needed for stable source symbols;
- the `require("<uri-pattern>")` DSL behavior;
- lazy resolution through the workspace instead of an implicit full project
  scan;
- a local/file-scoped smoke test for adding a command variant without its
  module;
- a repo-scoped smoke test for renaming a command module;
- documentation for the rule shape and the hook versus repo-scoped behavior.
