# Agent harness, hooks, and CI

`code-moniker check` is a normal command-line gate:

| Exit | Meaning |
| ---- | ------- |
| `0`  | pass |
| `1`  | rule violation or per-file read error during project scan |
| `2`  | usage or configuration error |

That makes it usable anywhere exit codes matter: editor hooks,
Claude Code `PostToolUse`, Git pre-commit, or CI.

For command behavior and rule syntax, see [`check`](check.md) and the
[Rule DSL](check-dsl.md).

## Install

```sh
cargo install code-moniker
```

From a local checkout:

```sh
cargo install --path crates/cli
```

Verify:

```sh
code-moniker langs
code-moniker check .
```

## Configure rules

Create `.code-moniker.toml` at the repository root.

```toml
[[refs.where]]
id      = "domain-no-infra"
expr    = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"
message = "Domain code must not depend on infrastructure."

[[ts.class.where]]
id      = "class-budget"
expr    = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` is too large for the project budget."
```

Inspect one file before writing path rules:

```sh
code-moniker extract src/order.ts --format tree
code-moniker extract src/order.ts --format json
```

The patterns in rules must match moniker segments such as `dir:domain`,
`package:com`, `module:order`, or `class:Order`.

## Claude Code `PostToolUse`

Run `check` after source edits by adding a hook to `.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "code-moniker check \"$CLAUDE_FILE_PATH\""
          }
        ]
      }
    ]
  }
}
```

Per-file checks are intended for this path:

- supported source files are checked;
- unsupported extensions return `0` with no output;
- project-wide scans should be left to pre-commit or CI.

Use suppressions for deliberate exceptions:

```ts
// code-moniker: ignore[domain-no-infra]
```

Put the suppression directly above the def it applies to. Use
`ignore-file[...]` only when the whole file is intentionally outside the
rule.

## Pre-commit

Create a repository script:

```bash
# scripts/check-arch.sh
#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
exec code-moniker check src/
```

Create a hook:

```bash
# .githooks/pre-commit
#!/usr/bin/env bash
set -euo pipefail

if git diff --cached --name-only --diff-filter=ACMR | grep -qE '^src/'; then
  ./scripts/check-arch.sh
fi
```

Enable it once per clone:

```sh
git config core.hooksPath .githooks
```

## CI

GitHub Actions example:

```yaml
name: architecture

on:
  pull_request:
  push:

jobs:
  code-moniker:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo install code-moniker
      - run: code-moniker check src/
```

For a workspace that already builds the local crate, avoid reinstalling:

```yaml
- run: cargo run -p code-moniker --bin code-moniker -- check src/
```

## Profiles

Profiles let hooks use different rule subsets.

```toml
[profiles.fast]
disable = ["\\.class-budget$"]

[profiles.release]
enable = ["^refs\\.", "^ts\\."]
```

```sh
code-moniker check src/ --profile fast
code-moniker check src/ --profile release
```

## Operational guidance

Keep per-edit rules local and fixable in one edit: naming, doc comments,
small class budgets, forbidden imports, direct layer boundaries.

Use project or CI scans for rules that need the whole tree. Use SQL over an
ingested `code_graph` corpus for transitive questions such as cycles,
indirect calls, or cross-repository dependency analysis.

When a rule unexpectedly misses, inspect the monikers with `extract` and
update the path pattern. Most misses are caused by using filesystem-style
paths where the graph uses typed segments.
