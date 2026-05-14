# `code-moniker check`

`check` evaluates a TOML rule pack against the symbol graph of one file or
one directory.

```sh
code-moniker check <PATH> [--rules <PATH>] [--format text|json] [--profile <NAME>] [--report]
code-moniker harness codex <ROOT> [--profile architecture] [--scope src]
code-moniker harness claude <ROOT> [--profile architecture] [--scope src]
```

Use it for local architecture checks, pre-commit hooks, CI jobs, or
per-file edit hooks. Use [`extract`](extract.md) when you only want to
inspect the graph.

Use `code-moniker harness codex` or `code-moniker harness claude` to
generate project-local `PostToolUse` hooks that run an architecture
profile after local write tools.

## Run

Check one file:

```sh
code-moniker check src/order.ts
```

Check a project:

```sh
code-moniker check src/
```

Use a rule file other than `.code-moniker.toml`:

```sh
code-moniker check src/ --rules arch.toml
```

Machine-readable output:

```sh
code-moniker check src/ --format json
```

Rule observability:

```sh
code-moniker check src/ --profile architecture --report
```

`--report` appends per-rule counts. For implication rules (`A => B`), it
also prints `antecedent_matches`; `0` means the left-hand side never
matched any scanned def or ref.

Exit codes:

| Code | Meaning |
| ---- | ------- |
| `0`  | no violations |
| `1`  | at least one violation, or a per-file read error during project scan |
| `2`  | usage error: bad path, invalid TOML, bad expression, unknown profile |

In single-file mode, unsupported extensions return `0` and produce no
output. This keeps edit hooks quiet for docs, configs, and generated files.

## Configuration

`check` always starts with the embedded default rule pack. If the rules
file exists, it is merged on top. The default path is `.code-moniker.toml`.

The embedded defaults cover conservative naming rules. Project policies
such as layer boundaries, maximum class size, or mandatory doc comments
belong in your overlay.

Minimal overlay:

```toml
[[refs.where]]
id      = "domain-no-infra"
expr    = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"
message = "Domain code must not depend on infrastructure."

[[ts.class.where]]
id      = "no-god-class"
expr    = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` exceeds the class budget."

[[ts.interface.where]]
id   = "repository-lives-in-domain"
expr = "name =~ Repository$ => moniker ~ '**/dir:domain/**'"
```

Rule ids are built from the TOML path: `ts.class.no-god-class`,
`refs.domain-no-infra`, and so on.

## Scopes

Each rule belongs to one scope:

| TOML path | Evaluated against |
| --------- | ----------------- |
| `[[<lang>.<kind>.where]]` | defs of one language and kind |
| `[[default.<kind>.where]]` | defs of one kind in any language, unless that language has its own kind block |
| `[[refs.where]]` | every ref in every supported language |
| `[[<lang>.refs.where]]` | refs emitted by one language |

Rust uses the TOML section `rust`, even though the language tag in
monikers and `code-moniker langs` is `rs`.

## Expressions

A rule expression is an assertion. If the assertion is false for the
current def or ref, `check` reports a violation.

Common def projections:

| Projection | Meaning |
| ---------- | ------- |
| `name` | bare name of the current def |
| `kind` | def kind such as `class`, `function`, `fn` |
| `shape` | cross-language group: `namespace`, `type`, `callable`, `value`, `annotation` |
| `visibility` | language visibility, when the extractor supports it |
| `lines` | line count for the def body |
| `moniker` | full moniker |
| `parent.name` | bare name of the parent segment |

Common ref projections:

| Projection | Meaning |
| ---------- | ------- |
| `kind` | ref kind such as `calls`, `imports_symbol`, `implements` |
| `source.name` / `target.name` | bare source or target name |
| `source.shape` / `target.shape` | source or target shape |
| `source ~ '...'` / `target ~ '...'` | path match against source or target moniker |

Operators:

| Operator | Meaning |
| -------- | ------- |
| `=` `!=` | equality |
| `<` `<=` `>` `>=` | numeric comparison |
| `=~` `!~` | regex match / no match |
| `~` | moniker path pattern match |
| `<@` `@>` | descendant / ancestor |
| `?=` | `bind_match`, used for cross-file symbol resolution |
| `AND` `OR` `NOT` `=>` | boolean logic; `A => B` means "when A, require B" |

Quantifiers:

```toml
count(method) <= 20
all(method, lines <= 60)
any(out_refs, kind = 'implements' AND target.name =~ Port$)
none(field, visibility = 'public')
```

Domains are direct child defs (`method`, `field`, `class`, etc.),
`segment`, `out_refs`, and `in_refs`.

Full grammar: [Rule DSL](check-dsl.md).

## Path patterns

Path patterns match moniker segments, not filesystem strings.

```toml
moniker ~ '**/dir:domain/**'
source  ~ '**/dir:application/**'
target  ~ '**/interface:/Port$/'
```

Project scans anchor each file at its path relative to the scanned root.
For example, `code-moniker check src/` sees `src/core/order.ts` as
`dir:core/module:order`, not `dir:src/dir:core/module:order`. If a layer
rule unexpectedly passes, run `check --report` and inspect the graph with
`extract --format tree`.

Language path encoding differs:

| Language | Path segments |
| -------- | ------------- |
| TS / JS / TSX / JSX | `dir:<segment>/module:<stem>` |
| Rust | `dir:<segment>/module:<stem>` |
| Go | `package:<segment>/module:<stem>` |
| C# | `package:<segment>/module:<stem>` |
| Java | `package:<segment>/module:<stem>` |
| Python | `package:<segment>/module:<stem>` |
| SQL / PL/pgSQL | `dir:<segment>/module:<stem>`, then `schema:<name>` nested under module for schema-scoped objects |

When a rule does not match what you expect, inspect the graph first:

```sh
code-moniker extract src/order.ts --format json
code-moniker extract src/order.ts --format tree
```

## Recipes

### Layer boundary

```toml
[[refs.where]]
id      = "domain-depends-only-on-domain"
expr    = "source ~ '**/dir:domain/**' => target ~ '**/dir:domain/**'"
message = "Domain code may only depend on domain code."

[[refs.where]]
id   = "application-depends-inward"
expr = """
  source ~ '**/dir:application/**'
  => target ~ '**/dir:application/**'
     OR target ~ '**/dir:domain/**'
"""
```

### Framework imports stay out of domain

```toml
[[refs.where]]
id   = "domain-imports-no-framework"
expr = """
  source ~ '**/dir:domain/**' AND kind = 'imports_symbol'
  => NOT (target ~ '**/external_pkg:express/**'
          OR target ~ '**/external_pkg:nestjs/**'
          OR target ~ '**/external_pkg:typeorm/**')
"""
```

### Keep classes small

```toml
[[ts.class.where]]
id      = "class-budget"
expr    = "count(method) <= 20 AND count(field) <= 7 AND all(method, lines <= 60)"
message = "Class `{name}` is too large for the project budget."
```

### DDD naming contracts

```toml
[[ts.class.where]]
id   = "entity-has-id"
expr = "name =~ Entity$ => any(field, name = 'id')"

[[ts.class.where]]
id   = "value-object-immutable"
expr = """
  (name =~ VO$ OR name =~ Value$)
  => all(field, visibility = 'private')
     AND none(method, name =~ ^set)
"""

[[ts.interface.where]]
id   = "repository-lives-in-domain"
expr = "name =~ Repository$ => moniker ~ '**/dir:domain/**'"
```

### Adapters implement ports

```toml
[[ts.class.where]]
id   = "adapter-implements-port"
expr = """
  name =~ Adapter$
  => any(out_refs, kind = 'implements'
                   AND target ~ '**/dir:application/**'
                   AND target.name =~ Port$)
"""
```

### Fixtures stay in tests

```toml
[[ts.class.where]]
id   = "fixtures-only-in-tests"
expr = """
  name =~ ^(Stub|Mock|Fake|Builder)$
  => any(segment, segment.kind = 'dir'
                  AND segment.name =~ (^tests?$|_test$))
"""
```

### Require doc comments

`require_doc_comment` is not a `where` expression. It is a field on a
kind block.

```toml
[ts.class]
require_doc_comment = "public"

[ts.interface]
require_doc_comment = "any"
```

Values are a visibility name such as `"public"` or `"private"`, plus the
special value `"any"`. A doc comment must end immediately before the
definition's doc anchor. Decorated definitions are handled by anchoring
before the decorator.

## Profiles

Profiles select a subset of rules by regex over full rule ids.

```toml
[profiles.bugfix]
enable = ["^ts\\.class\\.", "^refs\\.domain-no-infra$"]

[profiles.naming-only]
disable = ["\\.class-budget$", "\\.domain-"]
```

Run a profile:

```sh
code-moniker check src/ --profile bugfix
```

Selection rule:

```text
(enable is empty OR any enable pattern matches)
AND no disable pattern matches
```

Unknown profile names and bad regexes exit `2`.

## Suppressions

Suppress the next def:

```ts
// code-moniker: ignore
// code-moniker: ignore[ts.class.class-budget]
// code-moniker: ignore[class-budget]
```

Suppress a whole file:

```ts
// code-moniker: ignore-file
// code-moniker: ignore-file[class-budget]
```

The directive uses the language line-comment marker (`//`, `#`, or `--`).
Rule filters match by suffix, so `ignore[class-budget]` matches
`ts.class.class-budget`.

## Messages

Def-scoped rules support message templates:

| Token | Value |
| ----- | ----- |
| `{name}` | bare def name |
| `{kind}` | def kind |
| `{moniker}` | full def moniker |
| `{expr}` | raw expression |
| `{value}` | failing left-hand value |
| `{expected}` | right-hand literal |
| `{pattern}` `{lines}` `{limit}` `{count}` | aliases for `{expected}` or `{value}` |

Ref-scoped `message` values are emitted as literal explanatory text.

## Output

Text output prints one violation per line:

```text
src/widget.ts:L12-L18 [ts.class.name-pascalcase] class `lower_bad` fails `name =~ ^[A-Z][A-Za-z0-9]*$` (name = lower_bad, expected ^[A-Z][A-Za-z0-9]*$)
```

If a custom message is present, it is printed as the explanation below the
violation:

```text
src/widget.ts:L12-L18 [ts.class.name-pascalcase] class `lower_bad` fails `name =~ ^[A-Z][A-Za-z0-9]*$` (name = lower_bad, expected ^[A-Z][A-Za-z0-9]*$)
  → Class names must be PascalCase. Rename `lower_bad`.
```

Project scans end with a summary:

```text
3 violation(s) across 2 file(s) (42 scanned).
```

JSON output contains a `summary` object and one entry per scanned file.

## Next

- Inspect graphs with [`extract`](extract.md).
- Write exact expressions with the [Rule DSL](check-dsl.md).
- Wire `check` into hooks or CI with the [agent harness](agent-harness.md).
