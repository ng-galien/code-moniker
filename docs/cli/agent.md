# Agent integration, hooks, and CI

`code-moniker check` is a normal command-line gate:

| Exit | Meaning |
| ---- | ------- |
| `0`  | pass |
| `1`  | rule violation or per-file read error during project scan |
| `2`  | usage or configuration error |

That makes it usable anywhere exit codes matter: editor hooks, Codex or
Claude Code `PostToolUse`, Gemini CLI `AfterTool`, Git pre-commit, or CI.

For command behavior and rule syntax, see [`check`](check.md) and the
[Rule DSL](check-dsl.md).

## Hook filtering model

Generated hooks are edit-time filters, not full project scans.
After each matched write tool call, the hook:

1. reads the tool payload from `stdin`;
2. extracts the file paths touched by that tool call;
3. keeps only existing, supported source files;
4. runs one normal project-mode check on the configured scope with one
   `--file` flag per touched source file.

For a default install this is equivalent to:

```sh
code-moniker check --rules ".code-moniker.toml" "." --file "src/order.ts" --file "src/invoice.ts"
```

For a scoped, profiled install it is equivalent to:

```sh
code-moniker check --rules ".code-moniker.toml" --profile "agent" "src" --file "src/order.ts"
```

The `<scope>` argument still controls rule loading context, moniker anchors,
source-set heuristics, and what counts as in scope. The `--file` arguments
only filter the files extracted and evaluated for that hook invocation. If
the tool call touched no existing supported source files under the scope, the
hook exits `0` without running a broad check.

This keeps agent feedback fast while preserving the same behavior as the
corresponding project check for the files that were touched. Use pre-commit
or CI for full-tree guarantees.

## Install the agent integration

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/ng-galien/code-moniker/releases/latest/download/code-moniker-installer.sh | sh
code-moniker agent install --client codex
```

Official release binaries and normal source installs include MCP by default:
`cargo install code-moniker`. A deliberately minimal build can opt out with
`--no-default-features`, but it cannot provide agent MCP integration.

The binary embeds the version-matched `code-moniker` skill. `agent install`
materializes it in the selected client's user skill directory and registers a
project-owned stdio MCP using the canonical absolute project root.

The installed skill directory is a versioned Code Moniker artifact. Every
installation or update replaces that directory in full; it does not merge or
preserve local additions. Keep project-specific or personal instructions
outside the installed `code-moniker` skill directory.

For Codex, that MCP entry is deliberately registered with `required = false`.
Code Moniker enriches a session but a transient local startup failure must not
prevent the owning chat or delegated agent from starting; the client can report
the unavailable MCP and continue with its remaining capabilities.

```sh
code-moniker agent install --client codex
code-moniker agent install --client claude
code-moniker agent install --client gemini
```

The default components follow the binary capabilities: `skill,mcp` when the
binary contains MCP, otherwise `skill` only. Components can always be selected
explicitly with `--components`. Inspect or diagnose the managed integration
with:

```sh
code-moniker agent status --client codex
code-moniker agent doctor --client codex
code-moniker agent update --client codex
code-moniker agent uninstall --client codex
```

| Command | Behavior |
| ------- | -------- |
| `install` | Installs the explicit components, or the defaults supported by this binary. |
| `status` | Lists the tracked components and whether each is installed, external, outdated, stale, or missing. `outdated` means the current binary embeds a newer skill asset set than the tracked installation. |
| `doctor` | Checks component contents, embedded-skill checksum, and binary-version coherence; reports `skill update available` and exits non-zero when repair is needed. |
| `update` | Refreshes selected components from the current binary. With no `--components`, it uses the same capability-based defaults as `install`; hooks remain opt-in. Updating hooks reuses their recorded rules file, profile, scope, and violation limit. |
| `uninstall` | Removes selected managed components, or all tracked components when none are selected. |

The component scopes are deliberately different:

| Component | Scope | Installed by default |
| --------- | ----- | -------------------- |
| `skill` | User directory for the selected client | Yes |
| `mcp` | Project configuration, only when the binary has MCP support | With an MCP-enabled binary |
| `hooks` | Project-local write-time policy | No |

The concrete targets are:

| Client | User skill | Project MCP configuration | Project hooks |
| ------ | ---------- | ------------------------- | ------------- |
| Codex | `~/.codex/skills/code-moniker` | `.codex/config.toml` | `.codex/hooks/` and `.codex/hooks.json` |
| Claude | `~/.claude/skills/code-moniker` | `.mcp.json` | `.claude/hooks/` and `.claude/settings.json` |
| Gemini | `~/.gemini/skills/code-moniker` | `.gemini/settings.json` | `.gemini/hooks/` and `.gemini/settings.json` |

The installer records component ownership per canonical project root and
client under `~/.code-moniker/agent/`. This state lets lifecycle commands
distinguish content created by Code Moniker from matching configuration that
already existed. Lifecycle commands hold a client-scoped filesystem lock from
the state read through the last component mutation and state write, so two
concurrent commands cannot both succeed from the same stale state. The
user-scoped skill is shared by projects using the same client. Any tracked
project can update that shared skill. Each project keeps an independent
reference to the same physical installation; uninstall retains it while
another tracked project still references it and removes it with the last
managed reference.

`uninstall` removes only components recorded as managed and refuses to remove
an owned asset or configuration entry that has drifted since installation.
External components can always be forgotten without modifying their content.
When an install originally created a complete client configuration file,
uninstall removes that file only while its full checksum still matches the
exact content committed by Code Moniker. A later foreign setting invalidates
that whole-file ownership on the next install or update; uninstall then
removes only the Code Moniker registration and preserves the foreign content.
Directories created by the installer are removed only when they are still
empty.
Hook coherence covers both the generated script and its exact client
registration, while component versions are tracked independently so a partial
update remains visible to `doctor`. Pass
`--components skill`, `mcp`, or `hooks` to remove only one component.
An already matching physical skill or MCP entry discovered during installation
is recorded as external and retained by `uninstall`. Any skill symlink is
rejected rather than followed, materialized, or overwritten.
Symlinked parent directories or embedded skill assets are also rejected, so
installation and `doctor` only accept a fully physical skill layout.

Project hook installation follows the same physical-layout rule. Symlinked
client directories, hook directories, generated scripts, or hook configuration
files are rejected before any write. A later symlink drift makes the component
stale and blocks managed uninstall rather than following the link.
Project MCP configuration follows that rule as well: linked configuration
files or linked parent directories are rejected without modifying their
targets.
On the published macOS and Linux builds, reads and mutations are anchored to
the canonical project or home root with no-follow filesystem operations; a
root replaced by a symlink is rejected as drift. Conditional writes use an
atomic exchange (and conditional removes use an exclusive rename), verify the
inode installed by the rename, and restore a physical file from the captured
bytes and mode if a temporary name is substituted concurrently. A failed
integration-state write rolls hook, MCP, and skill mutations back. Physical
agent lifecycle operations fail closed on non-Unix platforms; those packages
are not published.

The skill adapts to the installed capabilities. It uses MCP tools when they are
available, the local binary when MCP is absent, and treats hooks as write-time
policy rather than a navigation surface.

Hooks are deliberately opt-in because they apply project rules:

```sh
code-moniker agent install --client codex --components hooks
```

This default hook runs `code-moniker check` without `--profile`. Select a
rules profile only through an explicit option:

```sh
code-moniker agent install \
  --client codex \
  --components hooks \
  --profile agent
```

From a local checkout, install a development binary first:

```sh
cargo install --path crates/cli
```

Verify:

```sh
code-moniker langs
code-moniker agent doctor --client codex
```

## Use cases

| Need | Use case | Configs shown |
| ---- | -------- | ------------- |
| Give Codex a live check hook from project rules | [Install a Codex live hook](#install-a-codex-live-hook) | `.code-moniker.toml`, `.codex/hooks.json`, `.codex/hooks/` |
| Give Claude Code the same project-local check hook | [Install a Claude Code live hook](#install-a-claude-code-live-hook) | `.code-moniker.toml`, `.claude/settings.json`, `.claude/hooks/` |
| Give Gemini CLI the same project-local check hook | [Install a Gemini CLI live hook](#install-a-gemini-cli-live-hook) | `.code-moniker.toml`, `.gemini/settings.json`, `.gemini/hooks/` |
| Stop the agent from adding prose comments inside Rust code | [Block prose comments inside code bodies](#block-prose-comments-inside-code-bodies) | `.code-moniker.toml`, `.claude/hooks/code-moniker-check.sh`, `.claude/settings.json` |
| Stop agent edits that cross a forbidden layer boundary | [Keep an agent inside a layer](#keep-an-agent-inside-a-layer) | `.code-moniker.toml`, `.claude/settings.json` |
| Make the agent split oversized TypeScript classes immediately | [Enforce small TypeScript classes after each edit](#enforce-small-typescript-classes-after-each-edit) | `.code-moniker.toml`, `.claude/settings.json` |
| Run a smaller rule set in edit hooks than in CI | [Run only fast edit-time rules for the agent](#run-only-fast-edit-time-rules-for-the-agent) | `.code-moniker.toml`, `.claude/settings.json`, CI command |
| Check the whole tree before commit | [Gate commits on agent guardrail rules](#gate-commits-on-agent-guardrail-rules) | `.code-moniker.toml`, `cargo moniker-check`, `.githooks/pre-commit` |
| Introduce rules in an existing repo without blocking everything | [Roll out rules in an existing repository](#roll-out-rules-in-an-existing-repository) | `.code-moniker.toml`, `.claude/settings.json`, non-blocking CI |

### Install a Codex live hook

Use this when Codex should run `code-moniker check` after local write-tool
edits. With no extra flags, the hook uses the project root as the rule
scope. The generated hook still filters each invocation to the files touched
by the Codex tool payload:

```sh
code-moniker agent install --client codex --components hooks .
```

That writes:

- `.codex/hooks/code-moniker-check.sh`
- `.codex/hooks.json`

Use `--profile` and `--check-scope` when you want a narrower, fast edit-time
rule set:

```toml
[profiles.agent]
enable = ["^architecture\\."]
```

Install project-local Codex configuration:

```sh
code-moniker agent install --client codex --components hooks . \
  --profile agent --check-scope src --max-violations 10
```

After installation, approve the project hook in Codex app Settings. This
approval is app-local state that the CLI cannot inspect, so `agent status` and
`agent doctor` report filesystem/configuration coherence but cannot confirm
that the app has enabled the hook.

Generated hooks pass one `--file` flag per touched source file and
`--max-violations 10` by default. The `--file` filtering keeps the hook from
rescanning the whole tree after every write tool call; `--max-violations`
keeps prompt feedback bounded by showing the first 10 violations from the
largest failed rule group, ordered by path and line. Use `--max-violations
N` at install time to change that limit.

When `--profile agent` is provided, the command verifies that
`[profiles.agent]` exists and names the hook from the profile:

- `.codex/hooks/code-moniker-agent.sh`
- `.codex/hooks.json`

Recommended Codex hook entry for the default install:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "apply_patch|Write|Edit|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "sh -c 'root=\"${CODEX_PROJECT_DIR:-$(pwd)}\"; exec \"$root/.codex/hooks/code-moniker-check.sh\"'"
          }
        ]
      }
    ]
  }
}
```

For a profiled install, the generated configuration points to the profiled
script name, for example `.codex/hooks/code-moniker-agent.sh`.

The generated script calls the binary directly. `--format codex-hook`
maps agent guardrail violations to Codex `PostToolUse` JSON feedback. Plain
text on `stdout` is ignored by Codex for this event, so failures are
emitted as a structured `decision: "block"` payload carrying the exact
`code-moniker check` diagnostics:

```sh
code-moniker check --rules ".code-moniker.toml" --format codex-hook --max-violations 10 "." --file "src/order.ts"
# with --profile agent --check-scope src:
code-moniker check --rules ".code-moniker.toml" --profile "agent" --format codex-hook --max-violations 10 "src" --file "src/order.ts"
```

The generated script records the absolute path of the `code-moniker` binary
that performed the installation. Re-run
`agent update --client codex --components hooks` after replacing or moving
that binary.

The default matcher covers local write tools only. MCP servers and custom
tools are outside the default guarantee boundary; add them explicitly only
after measuring their payload shape and cost. This live hook catches
agent-local writes early, but it is not a substitute for pre-commit hooks
or CI gates.

The generated script extracts touched files from Codex hook JSON by reading
`tool_input.command` for `apply_patch` and collecting `*** Add File`,
`*** Update File`, `*** Delete File`, and `*** Move to` patch headers.
It also accepts JSON operation shapes that expose `operation.path`.
Only the extracted touched files are passed as `--file`; if the payload does
not expose a source file path, the hook stays silent and exits `0`. Malformed
JSON is rejected instead of being treated as an empty file set.

Measure hook overhead on the target repository before enabling it for a team.
For a warm-cache edit hook, record at least the machine, scope, command, p50,
and p95 latency.

### Install a Claude Code live hook

Use this when Claude Code should run the same project-local check without
any global configuration writes. The generated script reads
`tool_input.file_path` from `Write`, `Edit`, and `MultiEdit` payloads and
passes each touched source file as `--file`.

```sh
code-moniker agent install --client claude --components hooks .
# or, for a named profile and narrower scope:
code-moniker agent install --client claude --components hooks . \
  --profile agent --check-scope src --max-violations 10
```

Generated hooks pass one `--file` flag per touched source file and
`--max-violations 10` by default. Use `--max-violations N` at install time
when a project needs a smaller or larger edit-time feedback window.

Without `--profile`, the command installs a root check:

- `.claude/hooks/code-moniker-check.sh`
- `.claude/settings.json`

When `--profile agent` is provided, the command verifies that
`[profiles.agent]` exists and names the hook from the profile:

- `.claude/hooks/code-moniker-agent.sh`
- `.claude/settings.json`

Recommended Claude Code hook entry for the default install:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "sh -c 'root=\"${CLAUDE_PROJECT_DIR:-$(pwd)}\"; exec \"$root/.claude/hooks/code-moniker-check.sh\"'"
          }
        ]
      }
    ]
  }
}
```

For a profiled install, the generated configuration points to the profiled
script name, for example `.claude/hooks/code-moniker-agent.sh`.

The generated script maps `code-moniker` violations to Claude's `exit 2`
feedback status and writes the diagnostic to `stderr`:

```sh
output=$(code-moniker check --rules ".code-moniker.toml" "." --file "$TOUCHED_FILE" 2>&1)
status=$?

if [ -n "$output" ] && [ "$status" -ne 0 ]; then
  printf '%s\n' "$output" >&2
fi

if [ "$status" -eq 1 ]; then
  exit 2
fi

exit "$status"
```

The generated script calls the absolute `code-moniker` path recorded during
installation. Re-run `agent update --client claude --components hooks` after
replacing or moving that binary.

`PostToolUse` runs after the edit is applied, so this is repair feedback
for the agent, not a guarantee that the write never happened. Keep
pre-commit and CI checks for repository guarantees.

### Install a Gemini CLI live hook

Use this when Gemini CLI should run the same project-local check after
tool edits. The generated script reads `tool_input.file_path` from
`write_file`, `replace`, and `edit` payloads and passes each touched source
file as `--file`.

```sh
code-moniker agent install --client gemini --components hooks .
# or, for a named profile and narrower scope:
code-moniker agent install --client gemini --components hooks . \
  --profile agent --check-scope src --max-violations 10
```

Generated hooks pass one `--file` flag per touched source file and
`--max-violations 10` by default. Gemini CLI project settings live in
`.gemini/settings.json`, and the generated hook is registered under
`hooks.AfterTool`.

Without `--profile`, the command installs a root check:

- `.gemini/hooks/code-moniker-check.sh`
- `.gemini/settings.json`

When `--profile agent` is provided, the command verifies that
`[profiles.agent]` exists and names the hook from the profile:

- `.gemini/hooks/code-moniker-agent.sh`
- `.gemini/settings.json`

Recommended Gemini CLI hook entry for the default install:

```json
{
  "hooks": {
    "AfterTool": [
      {
        "matcher": "write_file|replace|edit",
        "hooks": [
          {
            "name": "code-moniker-check",
            "type": "command",
            "command": "sh -c 'root=\"${GEMINI_PROJECT_DIR:-$(pwd)}\"; exec \"$root/.gemini/hooks/code-moniker-check.sh\"'"
          }
        ]
      }
    ]
  }
}
```

Gemini CLI hooks expect JSON on `stdout`; the generated script therefore
returns `{"decision":"allow"}` when `check` passes. When `check` reports
violations, the script writes the bounded diagnostics to `stderr` and exits
with code `2`, which Gemini CLI treats as a blocking hook failure.

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
output=$(cargo run --quiet -p code-moniker --bin code-moniker -- check . --file "$file_path" 2>&1)
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
            "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/code-moniker-check.sh"
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
            "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/code-moniker-check.sh"
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
            "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/code-moniker-agent-edit.sh"
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

### Gate commits on agent guardrail rules

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

`.cargo/config.toml`:

```toml
[alias]
moniker-check = "run --release -p code-moniker -- check ."
```

`.githooks/pre-commit`:

```bash
#!/usr/bin/env bash
set -euo pipefail

if git diff --cached --name-only --diff-filter=ACMR | grep -qE '^src/'; then
  cargo moniker-check
fi
```

Enable it once:

```sh
git config core.hooksPath .githooks
```

### Roll out rules in an existing repository

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
            "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/code-moniker-agent-edit.sh"
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

Non-blocking CI audit while the cleanup is in progress:

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
            "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/code-moniker-check.sh"
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

Add a cargo alias:

```toml
# .cargo/config.toml
[alias]
moniker-check = "run --release -p code-moniker -- check src/"
```

Create a hook:

```bash
# .githooks/pre-commit
#!/usr/bin/env bash
set -euo pipefail

if git diff --cached --name-only --diff-filter=ACMR | grep -qE '^src/'; then
  cargo moniker-check
fi
```

Enable it once per clone:

```sh
git config core.hooksPath .githooks
```

## CI

GitHub Actions example:

```yaml
name: agent

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

Profiles let hooks use different subsets of the rules that `check` already
loaded. They do not toggle the embedded default rules; use
`default_rules = false`, `code-moniker rules disable`, or
`--default-rules off` for that.

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

Profile `enable` and `disable` values are regexes over full rule ids such
as `refs.domain-no-infra` or `ts.class.class-budget`. If `enable` is empty,
all loaded rules are candidates. If `enable` is present, only matching
rules are candidates. `disable` then removes matching candidates.

## Operational guidance

Keep per-edit rules local and fixable in one edit: naming, doc comments,
small class budgets, forbidden imports, direct layer boundaries.

Use project or CI scans for rules that need the whole tree. Use SQL over an
ingested `code_graph` corpus for transitive questions such as cycles,
indirect calls, or cross-repository dependency analysis.

When a rule unexpectedly misses, inspect the monikers with `extract` and
update the path pattern. Most misses are caused by using filesystem-style
paths where the graph uses typed segments.
