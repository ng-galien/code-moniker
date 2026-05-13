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

## Use cases

| Need | Use case | Configs shown |
| ---- | -------- | ------------- |
| Stop the agent from adding prose comments inside Rust code | [Block prose comments inside code bodies](#block-prose-comments-inside-code-bodies) | `.code-moniker.toml`, `.claude/hooks/code-moniker-check.sh`, `.claude/settings.json` |
| Stop agent edits that cross a forbidden layer boundary | [Keep an agent inside a layer](#keep-an-agent-inside-a-layer) | `.code-moniker.toml`, `.claude/settings.json` |
| Make the agent split oversized TypeScript classes immediately | [Enforce small TypeScript classes after each edit](#enforce-small-typescript-classes-after-each-edit) | `.code-moniker.toml`, `.claude/settings.json` |
| Run a smaller rule set in edit hooks than in CI | [Run only fast edit-time rules for the agent](#run-only-fast-edit-time-rules-for-the-agent) | `.code-moniker.toml`, `.claude/settings.json`, CI command |
| Check the whole tree before commit | [Gate commits on architecture rules](#gate-commits-on-architecture-rules) | `.code-moniker.toml`, `scripts/check-arch.sh`, `.githooks/pre-commit` |
| Introduce rules in a legacy repo without blocking everything | [Roll out rules in a legacy repository](#roll-out-rules-in-a-legacy-repository) | `.code-moniker.toml`, `.claude/settings.json`, non-blocking CI |

### Block prose comments inside code bodies

Use this when the agent keeps adding explanatory comments inside functions,
methods, structs, enums, traits, or impls. The project allows comments at
module boundaries, in tests/examples, and for explicit `SAFETY:` notes.

`.code-moniker.toml`:

```toml
[aliases]
tests   = "moniker ~ '**/dir:tests/**'"
example = "moniker ~ '**/dir:examples/**'"

[[rust.comment.where]]
id      = "no-nested-comments"
expr    = "$tests OR $example OR parent.kind = 'module' OR text =~ '^//\\s*SAFETY:'"
message = "Do not add comments inside functions, methods, structs, enums, traits, or impls. Keep code self-explanatory; only module-boundary and `SAFETY:` comments are allowed."
```

`.claude/hooks/code-moniker-check.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

file_path=$(jq -r '.tool_input.file_path // empty' 2>/dev/null || true)
[ -n "$file_path" ] || exit 0
[ -f "$file_path" ] || exit 0

root="${CLAUDE_PROJECT_DIR:-$(pwd)}"
cd "$root"

set +e
output=$(cargo run --quiet -p code-moniker --bin code-moniker -- check "$file_path" 2>&1)
status=$?
set -e

if [ "$status" -ne 0 ]; then
  {
    echo "$output"
    if [ "$status" -eq 1 ]; then
      echo
      echo "code-moniker blocked this write. Fix every reported violation in this file."
    fi
  } >&2
  exit 2
fi
```

`.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/code-moniker-check.sh"
          }
        ]
      }
    ]
  }
}
```

### Keep an agent inside a layer

Use this when an edit hook should immediately reject a dependency from
`domain/` to `infrastructure/`.

`.code-moniker.toml`:

```toml
[[refs.where]]
id      = "domain-no-infra"
expr    = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"
message = "Domain code must not depend on infrastructure."
```

`.claude/settings.json`:

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

### Enforce small TypeScript classes after each edit

Use this when the agent should split oversized classes before moving on.

`.code-moniker.toml`:

```toml
[[ts.class.where]]
id      = "class-budget"
expr    = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` is too large for the project budget."
```

`.claude/settings.json`:

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

### Run only fast edit-time rules for the agent

Use this when the project has strict CI rules, but the edit hook should only
run rules that are easy to fix in one file.

`.code-moniker.toml`:

```toml
[[refs.where]]
id      = "domain-no-infra"
expr    = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"
message = "Domain code must not depend on infrastructure."

[[ts.class.where]]
id      = "class-budget"
expr    = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` is too large for the project budget."

[profiles.agent-edit]
enable = [
  "^refs\\.domain-no-infra$",
  "^ts\\.class\\.name-pascalcase$",
  "^ts\\.function\\.name-camelcase$"
]

[profiles.full]
enable = ["^refs\\.", "^ts\\."]
```

`.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "code-moniker check \"$CLAUDE_FILE_PATH\" --profile agent-edit"
          }
        ]
      }
    ]
  }
}
```

CI can use the full profile:

```sh
code-moniker check src/ --profile full
```

### Gate commits on architecture rules

Use this when per-edit feedback is too narrow and every commit should check
the whole source tree.

`.code-moniker.toml`:

```toml
[[refs.where]]
id   = "application-depends-inward"
expr = """
  source ~ '**/dir:application/**'
  => target ~ '**/dir:application/**'
     OR target ~ '**/dir:domain/**'
"""

[[refs.where]]
id   = "domain-depends-only-on-domain"
expr = "source ~ '**/dir:domain/**' => target ~ '**/dir:domain/**'"
```

`scripts/check-arch.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
exec code-moniker check src/
```

`.githooks/pre-commit`:

```bash
#!/usr/bin/env bash
set -euo pipefail

if git diff --cached --name-only --diff-filter=ACMR | grep -qE '^src/'; then
  ./scripts/check-arch.sh
fi
```

Enable it once:

```sh
git config core.hooksPath .githooks
```

### Roll out rules in a legacy repository

Use this when existing code violates the full policy, but new agent edits
should still obey a small subset.

`.code-moniker.toml`:

```toml
[[refs.where]]
id      = "domain-no-infra"
expr    = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"
message = "Domain code must not depend on infrastructure."

[[ts.class.where]]
id      = "class-budget"
expr    = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` is too large for the project budget."

[profiles.agent-edit]
enable = ["^refs\\.domain-no-infra$"]

[profiles.report-only]
enable = ["^refs\\.", "^ts\\."]
```

Agent hook:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "code-moniker check \"$CLAUDE_FILE_PATH\" --profile agent-edit"
          }
        ]
      }
    ]
  }
}
```

Local audit command:

```sh
code-moniker check src/ --profile report-only --format json
```

Non-blocking CI audit while the legacy cleanup is in progress:

```yaml
- name: code-moniker report
  run: code-moniker check src/ --profile report-only --format json
  continue-on-error: true
```

## Writing the first rule

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
