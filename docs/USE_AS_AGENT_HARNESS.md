# Use `code-moniker` as an agent harness

`code-moniker check` exits 0 when the codebase satisfies every rule,
1 when at least one rule fires, 2 on a usage error. Anything that
reads exit codes — a Claude Code `PostToolUse` hook, a Git
pre-commit hook, a CI job — can gate on that signal.

Reference: [`CLI_CHECK.md`](CLI_CHECK.md) (subcommand), [`CHECK_DSL.md`](CHECK_DSL.md) (rule grammar).

## What rules talk about

Rules operate on the symbol graph extracted from source: defs,
refs, imports, inheritance, calls. They are not regex on text. A
rule can require that a class under `src/domain/` (encoded as
`dir:src/dir:domain` in the moniker) never imports from
`src/infrastructure/`, that every `*Repository` interface lives
under the domain directory, that no class has more than 20
methods, that every public method has a doc-comment on the line
above.

Path encoding depends on the language: TS / JS / Rust / Go / C#
use `dir:<segment>`, Java and Python use `package:<segment>`, SQL
uses `schema:<name>`. Patterns in rules must match the encoding
the extractor produces (run `code-moniker <file> --format json`
to see one).

## Install

```sh
cargo install --git https://github.com/ng-galien/code-moniker --features cli code-moniker
```

In-tree alternative: `cargo run --features cli --bin code-moniker -- check src/`.

Supported languages: TypeScript / JavaScript / TSX / JSX, Rust, Java,
Python, Go, C#, and PL/pgSQL when built with `--features pg17`.

## Configure

Drop `.code-moniker.toml` at the repo root. An empty file is valid:
the embedded default rule pack covers naming hygiene, doc-comments
on public symbols, god classes, and deep nesting. User entries
overlay the default by rule id.

```toml
[[refs.where]]
id      = "domain-no-infra"
expr    = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"
message = "Domain code in `{moniker}` reaches into infrastructure (`{value}`)."

[[ts.class.where]]
id   = "no-god-class"
expr = "count(method) <= 20 AND all(method, lines <= 60)"

[[ts.interface.where]]
id   = "repository-lives-in-domain"
expr = "name =~ Repository$ => moniker ~ '**/dir:domain/**'"
```

Full grammar: [`CHECK_DSL.md`](CHECK_DSL.md). A larger example
covering Clean Code, DDD, Hex layering, and bounded contexts is
appended to that file.

## Run

```sh
$ code-moniker check src/
src/domain/order.ts:L42-L88 [ts.class.no-god-class] class `Order` fails `count(method) <= 20`
  → Class `Order` is too wide (24).
1 violation(s) across 1 file(s) (47 scanned).
$ echo $?
1
```

`--format json` switches to structured output for harnesses that
parse the report.

## Claude Code `PostToolUse` hook

Claude Code fires `PostToolUse` hooks after `Edit` / `Write` /
`MultiEdit`. A non-zero exit becomes feedback in the conversation,
which lets the agent read its own violation and self-correct.

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

| Exit | Effect on the agent                                                                              |
| ---- | ------------------------------------------------------------------------------------------------ |
| 0    | Silent; the edit proceeds.                                                                       |
| 1    | Violation text injected into the conversation. The agent reads it and re-tries.                  |
| 2    | Silent (the hook is a no-op for non-source edits and parse errors).                              |

The agent sees the rule id, the violating moniker, and the custom
`message` (with `{name}`, `{value}`, `{moniker}` substituted). The
rule's `explanation` carries the remediation hint.

Operational guidance:

- `$CLAUDE_FILE_PATH` is the file just edited. Per-file mode is
  fast and bounded; project-wide scans (`check src/`) belong in
  pre-commit / CI.
- For one-off legitimate exceptions, add
  `// code-moniker: ignore[<rule-id>]` on the line above the def.
  The audit trail stays in the source.
- A violation should describe a fix the agent can apply in the same
  edit (rename, split, move). Rules that require coordinated changes
  across multiple files will trap the agent in a loop.

## Pre-commit + CI gate

```bash
# scripts/check-arch.sh
#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
exec code-moniker check src/
```

```bash
# .githooks/pre-commit (excerpt)
if git diff --cached --name-only --diff-filter=ACMR | grep -qE '^src/'; then
    ./scripts/check-arch.sh || {
        echo "pre-commit: architecture lint failed."
        echo "  Suppress with // code-moniker: ignore[<rule-id>] if the exception is legitimate."
        exit 1
    }
fi
```

Activate the hook once per clone: `git config core.hooksPath .githooks`.

```yaml
# .github/workflows/ci.yml (excerpt)
arch-check:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
      with:
        shared-key: cli
    - run: ./scripts/check-arch.sh
```

Git rejects the commit on exit 1; GitHub Actions fails the job.
Exit 2 (usage error) should also fail the job — it signals a
malformed config, not a code-style violation.

## Rule patterns

### Naming hygiene per language and kind

```toml
[[rust.fn.where]]
id      = "max-lines"
expr    = "lines <= 120"
message = "Function `{name}` is {value} lines (project cap = {expected})."

[[rust.trait.where]]
id      = "no-i-prefix"
expr    = "NOT name =~ ^I[A-Z]"
```

### Cross-module / cross-layer dependencies

```toml
[aliases]
core = "moniker ~ '**/dir:core/**'"
pg   = "moniker ~ '**/dir:pg/**'"
lang = "moniker ~ '**/dir:lang/**'"

[[refs.where]]
id      = "core-no-pgrx"
expr    = "$core AND kind = 'imports_symbol' => NOT target ~ '**/external_pkg:pgrx/**'"
message = "Module `core` is parser-agnostic; pgrx imports belong under `src/pg/`."
```

### Architectural invariants

```toml
[[refs.where]]
id   = "domain-depends-on-nothing-but-itself-or-std"
expr = """
  source ~ '**/dir:domain/**'
  => target ~ '**/dir:domain/**'
     OR target ~ '**/external_pkg:std/**'
"""

[[ts.class.where]]
id   = "use-case-shape"
expr = """
  name =~ UseCase$
  => count(method) = 1 AND any(method, name = 'execute')
"""
```

## Beyond per-file rules

Rules see direct refs of the current def. Transitive analysis
(`X indirectly calls Y`), cycle detection, and dataflow live in SQL
against an ingested corpus — see [`USE_IN_POSTGRES.md`](USE_IN_POSTGRES.md).
