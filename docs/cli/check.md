# `code-moniker check`

`check` evaluates a TOML rule pack against the symbol graph of one file or
one directory.

```sh
code-moniker check <PATH> [--file <PATH>]... [--rules <PATH>] [--default-rules on|off] [--format text|json|codex-hook] [--profile <NAME>] [--report] [--max-violations <N>]
code-moniker rules init [ROOT] [--rules <PATH>]
code-moniker rules disable [ROOT] [--rules <PATH>]
code-moniker rules enable [ROOT] [--rules <PATH>]
code-moniker rules show [ROOT] [--rules <PATH>] [--profile <NAME>] [--default-rules on|off] [--format text|json]
code-moniker rules learn [SAMPLE] [--format text|json]
code-moniker harness codex [ROOT] [--profile <NAME>] [--scope <PATH>] [--max-violations <N>]
code-moniker harness claude [ROOT] [--profile <NAME>] [--scope <PATH>] [--max-violations <N>]
code-moniker harness gemini [ROOT] [--profile <NAME>] [--scope <PATH>] [--max-violations <N>]
```

Use it for local architecture checks, pre-commit hooks, CI jobs, or
per-file edit hooks. Use [`extract`](extract.md) when you only want to
inspect the graph.

Use `code-moniker harness codex`, `code-moniker harness claude`, or
`code-moniker harness gemini` to generate project-local hooks that run
`code-moniker check` after local write tools. By default, harnesses check
`.` with the root `.code-moniker.toml`; use `--profile` and `--scope` for
a narrower edit-time rule set.

## Mental model

`check` builds the same symbol graph as `extract`, loads the rule config,
and evaluates each rule on one scope: one def, one ref, or one direct child
collection. By default, the rule config is the embedded defaults merged
with your TOML overlay; `--default-rules off` makes the TOML file the
complete config.

A `where` expression is an assertion. When it evaluates to `false`,
`check` emits a violation. Most architecture rules should therefore use
implication: `A => B` means "when A is true, B must also be true". Without
the implication, a rule that starts with `A AND ...` will also fail every
symbol where `A` is false.

## Run

Check one file:

```sh
code-moniker check src/order.ts
```

Check a project:

```sh
code-moniker check src/
```

Check only files touched by an edit hook, while keeping project-mode
moniker anchors and rule behavior:

```sh
code-moniker check . --file src/order.ts --file src/invoice.ts
```

`--file` is a directory-scan filter, not a replacement for `<PATH>`.
This is the mode generated live harnesses use after Codex, Claude Code, or
Gemini write tools. The command loads rules exactly like a normal project
check on `<PATH>`, including profile handling and project/source-set
heuristics, but it only extracts and evaluates supported source files named
by `--file` that still exist under the checked directory. Multiple touched
files become multiple `--file` flags. Unsupported, missing, or out-of-scope
touched files produce no output and exit `0`, matching edit-hook behavior.
Rules that use `require(...)` may lazily inspect the concrete target file
derived from the current symbol, but `--file` does not become a full
workspace scan.

For example, a harness installed with `--scope src --profile architecture`
runs the equivalent of:

```sh
code-moniker check --profile architecture src --file src/order.ts
```

That keeps the `src` project scope and rule behavior, while filtering the
hook invocation to `src/order.ts`.

Use a rule file other than `.code-moniker.toml`:

```sh
code-moniker check src/ --rules arch.toml
```

Run only the rules from your project file:

```sh
code-moniker check src/ --rules arch.toml --default-rules off
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

Debug a rule that does not fire in this order:

```sh
code-moniker check src/ --profile architecture --report
code-moniker extract src/domain/order.ts --format tree
code-moniker langs ts
```

Use `--report` to see whether the rule was evaluated, `extract` to verify
the real moniker/kind names, and `langs` to confirm the kinds emitted by a
language.

Limit text feedback when a repository has many violations:

```sh
code-moniker check src/ --profile architecture --max-violations 10
```

`--max-violations` keeps the summary and failed-rule counts complete, but
prints only violations from the largest failed rule group. Within that
group, entries are ordered by path and line, then the first `N` are shown.
Generated agent harnesses pass `--max-violations 10` by default so edit
feedback stays small enough for an agent prompt.

Exit codes:

| Code | Meaning |
| ---- | ------- |
| `0`  | no error-severity violations; warning-severity violations may be present |
| `1`  | at least one error-severity violation, or a per-file read error during project scan |
| `2`  | usage error: bad path, invalid TOML, bad expression, unknown profile |

`--format codex-hook` is the exception: rule failures are emitted as a
Codex block payload on stdout and the process exits `0`; usage errors still
exit `2`.

In single-file mode, unsupported extensions return `0` and produce no
output. This keeps edit hooks quiet for docs, configs, and generated files.

## Configuration

By default, `check` starts with the embedded default rule pack. If the
rules file exists, it is merged on top. The default path is
`.code-moniker.toml`.

Use `code-moniker rules init` to create `.code-moniker.toml` at the project
root. It detects common project manifests such as `pom.xml`, `Cargo.toml`,
`package.json`, `pyproject.toml`, `go.mod`, and `*.csproj`, then seeds a
small `[aliases]` block for path-oriented project rules.

### Embedded defaults

`default_rules` controls only the embedded default rule pack shipped in the
binary. It does not enable or disable rules written in your
`.code-moniker.toml`.

| Setting | Effect |
| ------- | ------ |
| missing / `true` | run embedded defaults, then merge project rules on top |
| `false` | run only rules from `.code-moniker.toml` |
| `--default-rules on` | force embedded defaults for this invocation |
| `--default-rules off` | force project-only rules for this invocation |

Use `--default-rules off` when the TOML file should be the complete rule
set. If the rules file is missing in that mode, no rules run.

The same behavior can be stored in the rules file:

```toml
default_rules = false
```

`code-moniker rules disable` writes `default_rules = false`.
`code-moniker rules enable` writes `default_rules = true`.

```sh
code-moniker rules disable .
code-moniker check .
# project rules only

code-moniker rules enable .
code-moniker check .
# embedded defaults + project rules

code-moniker check . --default-rules off
# project rules only, even if the file says default_rules = true
```

An explicit command-line `--default-rules on` or `--default-rules off` wins
for that invocation. The `rules enable` / `rules disable` commands do not
touch `[profiles.*]`; they only update the top-level `default_rules` flag.

### Global exclusions

Use `[exclude].uris` to keep files outside a rule pack's review surface.
In CLI project checks, excluded files are skipped before extraction and
rule evaluation. Patterns are slash-normalized URI globs matched against
the file URI and normalized filesystem path. `*` matches one path segment,
`?` matches one character inside a segment, and `**` matches across path
segments.

```toml
[exclude]
uris = [
  "**/crates/core/tests/fixtures/**",
]
```

Excluded files are not counted in `summary.files_scanned`, do not appear in
the JSON `files` array, and do not produce read errors during CLI checks.
In the TUI, exclusions apply to the check summary for the already loaded
workspace graph. This is intended for generated sources, vendored trees,
fixtures, and other files that should be outside a rule pack's review
surface.

Use `rules show` when you need to see what `check` will actually run after
loading embedded defaults, merging the project file, applying the optional
profile, resolving aliases, and compiling expressions:

```sh
code-moniker rules show .
code-moniker rules show . --profile agent-edit
code-moniker rules show . --default-rules off --format json
```

Text output groups compiled rules by language. JSON output includes
`expr`, `expanded_expr`, and optional `rationale`, so alias expansion and
rule intent are visible without running a check.

### Rule fragments

Large projects can split local rule policy into colocated fragments. The
root `.code-moniker.toml` stays the global entrypoint; when it exists,
`check` also discovers every `code-moniker.fragment.toml` below the same
directory and merges enabled fragments after the root file.

Each fragment must declare a stable id:

```toml
fragment = "check"
enabled = true # optional; defaults to true

[aliases]
expr_parse = "moniker ~ '**/dir:check/dir:expr/module:parse/**'"

[[refs.where]]
id      = "parse-no-eval"
expr    = "$expr_parse => NOT target ~ '**/module:eval/**'"
message = "`check::expr::parse` must stay parser-only."
```

Fragment rule ids are local in the file and must be explicit. At merge
time, the fragment id is injected into the effective rule id. The example
above becomes `refs.check.parse-no-eval`. Fragments cannot override root
rules or each other; a duplicate effective rule id is a config error.

Fragment aliases are local for readability. `$expr_parse` inside fragment
`check` is stored as the effective alias `check_expr_parse`; local aliases
can reference global aliases and other local aliases, but a fragment alias
cannot shadow an existing alias or collide with an effective alias name.
Aliases shared by more than one fragment belong in the root file. Alias
names in fragments use ASCII letters, digits, and `_`.

`enabled = false` keeps the fragment discoverable but does not merge its
rules or aliases. `rules show` lists every declared fragment and reports
`active_rules` separately from `declared_rules`, so disabled fragments and
profile-filtered rules do not disappear silently.

Use `rules learn` to print the example rule packs embedded in the binary.
This is intended for agents and local tooling: they can inspect language
and architecture examples without fetching docs from GitHub or depending
on a checkout of the repository.

```sh
code-moniker rules learn
code-moniker rules learn java
code-moniker rules learn architecture --format json
```

Known samples are `architecture`, `csharp`, `go`, `java`, `python`,
`rust`, `sql`, and `typescript`.

The embedded defaults cover conservative naming rules. Project policies
such as layer boundaries, maximum class size, or mandatory doc comments
belong in your overlay.

Minimal overlay:

```toml
[[refs.where]]
id      = "domain-no-infra"
expr    = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"
message = "Domain code must not depend on infrastructure."
rationale = """
ADR-003: domain code is the stable core of the application.
Infrastructure changes must not force domain changes.
"""

[[ts.class.where]]
id      = "no-god-class"
expr    = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` exceeds the class budget."

[[ts.interface.where]]
id   = "repository-lives-in-domain"
severity = "warn"
expr = "name =~ Repository$ => moniker ~ '**/dir:domain/**'"
```

Rule ids are built from the TOML path: `ts.class.no-god-class`,
`refs.domain-no-infra`, and so on.

`message` is the short diagnostic shown with a violation. `severity` is
optional and accepts `"error"` or `"warn"`; omitted rules default to
`"error"`. Warning rules are reported in text and JSON output but do not
make `check` exit `1` by themselves. `rationale` is optional rule metadata
for the architectural decision behind the rule; it is shown by `rules show`
but not by `check` violation output.

The three examples above cover the common rule shapes:

| Rule | What it checks |
| ---- | -------------- |
| `refs.domain-no-infra` | a direct dependency from one layer to another |
| `ts.class.no-god-class` | a class budget using direct child methods |
| `ts.interface.repository-lives-in-domain` | a naming convention tied to location |

## Scopes

Each rule belongs to one scope:

| TOML path | Evaluated against |
| --------- | ----------------- |
| `[[<lang>.<kind>.where]]` | defs of one language and kind |
| `[[<lang>.shape.<shape>.where]]` | defs of one language and canonical shape |
| `[[shape.<shape>.where]]` | defs of one canonical shape in any language |
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
| `uri` | full moniker URI; prefer this spelling in new rules |
| `moniker` | compatibility alias for `uri` |
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
| `subset` | multiset containment |
| `AND` `OR` `NOT` `=>` | boolean logic; `A => B` means "when A, require B" |

Quantifiers:

```toml
count(method) <= 20
all(method, lines <= 60)
any(out_refs, kind = 'implements' AND target.name =~ Port$)
count(shape:callable) <= 20
count(pairs(method), a.name = b.name) = 0
none(field, visibility = 'public')
```

Domains are direct child defs (`method`, `field`, `class`, etc.),
direct child defs by shape (`shape:callable`, `shape:value`, etc.),
`segment`, `out_refs`, and `in_refs`. `pairs(D)` is supported by
`count`, `any`, `all`, and `none`.

The rule language also supports local numeric analytics:

```toml
cv(method, fan_out(each)) <= 0.6
size(unique(method.name)) = size(method.name)
field.name subset method.name
lcom4(self) <= 1
cbo(self) <= 14 AND rfc(self) <= 50 AND wmc(self) <= 47
```

Full grammar: [Rule DSL](check-dsl.md).

## Path patterns

Path patterns match moniker segments, not filesystem strings.

```toml
uri     ~ '**/dir:domain/**'
source  ~ '**/dir:application/**'
target  ~ '**/interface:/Port$/'
```

Use `require("<uri-pattern>")` when the current symbol implies that another
symbol or resource must exist. The pattern can use `{name}` and
`{name.snake}` placeholders from the current def:

```toml
uri ~ '**/module:args/enum:Command' => all(enum_constant,
  require("**/dir:crates/dir:cli/dir:src/dir:{name.snake}/module:mod")
)
```

This example says that each `args::Command` enum constant must have a
matching command module directory. In file-scoped hook checks, `require`
resolves only the derived target file when possible; repo-scoped checks
remain the broader safety net.

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

The recipes cover direct dependency boundaries, external framework imports,
class-size budgets, naming/location contracts, implementation contracts,
test-only fixtures, doc comments, profiles, and suppressions. The DSL does
not compute transitive dependency closure or cycles; use SQL over an
ingested `code_graph` for those corpus-level checks.

For copyable language-specific starting points, see the commented TOML
samples:

| Language | Sample |
| -------- | ------ |
| Architecture patterns | [architecture.toml](check-samples/architecture.toml) |
| Test guardrails | [test-guardrails.toml](check-samples/test-guardrails.toml) |
| TypeScript / JavaScript | [typescript.toml](check-samples/typescript.toml) |
| Rust | [rust.toml](check-samples/rust.toml) |
| Java | [java.toml](check-samples/java.toml) |
| Python | [python.toml](check-samples/python.toml) |
| Go | [go.toml](check-samples/go.toml) |
| C# | [csharp.toml](check-samples/csharp.toml) |
| SQL / PL/pgSQL | [sql.toml](check-samples/sql.toml) |

Literature-inspired samples encode structural rules from canonical software
engineering literature. They are community-authored examples; attribution and
non-endorsement notes sit at the top of each file.

| Source | Sample |
| ------ | ------ |
| Robert C. Martin, *Clean Architecture* (2017) | [clean-architecture.toml](check-samples/clean-architecture.toml) |
| Martin Fowler, *Patterns of Enterprise Application Architecture* (2002) | [fowler-eaa.toml](check-samples/fowler-eaa.toml) |
| Martin Fowler, *Refactoring* (1999/2018) | [fowler-refactoring.toml](check-samples/fowler-refactoring.toml) |

See [Code smell review](code-smell-review.md) for the executable local
coverage model, current operator gaps, and the warning-first review
workflow.

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

This catches direct refs from `application` to forbidden layers, or from
`domain` to anything outside `domain`. It does not flag an indirect path
such as `domain -> application -> infrastructure`.

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

This rule is intentionally scoped to `imports_symbol` refs. Method calls to
framework objects already imported elsewhere need a separate rule or a SQL
query over the graph.

### Spring proxy self-invocation

Spring AOP advice in proxy mode only runs when a call enters through the
proxy. A same-class call to a `@Transactional`, `@Async`, `@Cacheable`, or
method-security annotated method bypasses the advice even though the code
looks like an ordinary method call.

This is a useful check example because the mistake is not local to one
syntax node. The executable Java sample is the copy-paste source of truth:
[check-samples/java.toml](check-samples/java.toml). It contains the
method-level and class-level proxy checks with the same annotation set.

Those rules first select proxy-advised declarations from annotation refs,
then inspect incoming call refs, and finally compare caller/callee parent
monikers to distinguish same-class calls from normal calls through another
component. Projects using AspectJ weaving or deliberate self-injected proxy
references should relax or remove these rules.

Source references:

- Spring AOP proxying and self-invocation:
  <https://docs.spring.io/spring-framework/reference/core/aop/proxying.html>
- Declarative transaction annotations:
  <https://docs.spring.io/spring-framework/reference/data-access/transaction/declarative/annotations.html>
- Cache annotations and proxy/aspectj mode:
  <https://docs.spring.io/spring-framework/reference/integration/cache/annotations.html>
- Async annotation support:
  <https://docs.spring.io/spring-framework/reference/integration/scheduling.html#scheduling-annotation-support-async>
- Spring Security method security annotations:
  <https://docs.spring.io/spring-security/reference/servlet/authorization/method-security.html>
- Spring Framework resilience annotations:
  <https://docs.spring.io/spring-framework/reference/core/resilience.html>

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
  name =~ (Stub|Mock|Fake|Builder)$
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

Profiles select a subset of the already-loaded rules. They do not decide
whether embedded defaults are loaded; `default_rules` and
`--default-rules` do that first.

Evaluation order:

1. Load embedded defaults unless disabled.
2. Merge `.code-moniker.toml` on top.
3. Discover and merge enabled `code-moniker.fragment.toml` files below the
   rules file directory.
4. If `--profile <name>` is passed, filter the resulting rule set.

Profiles use regexes over full rule ids. Full ids are built from the TOML
path:

| TOML rule | Full id |
| --------- | ------- |
| `[[refs.where]] id = "domain-no-infra"` | `refs.domain-no-infra` |
| `[[ts.class.where]] id = "class-budget"` | `ts.class.class-budget` |
| `[[shape.callable.where]] id = "max-lines"` | `shape.callable.max-lines` |
| `[[rust.shape.callable.where]] id = "max-lines"` | `rust.shape.callable.max-lines` |
| `[[default.module.where]] id = "max-lines"` | `default.module.max-lines` |
| `[[java.refs.where]] id = "no-spring"` | `java.refs.no-spring` |
| `fragment = "ui"` + `[[refs.where]] id = "store-boundary"` | `refs.ui.store-boundary` |

Rules without an `id` get generated ids such as `where_0`, but profiles
are clearer and more stable when every profiled rule has an explicit `id`.

```toml
[profiles.bugfix]
enable = ["^ts\\.class\\.", "^refs\\.domain-no-infra$"]

[profiles.naming-only]
disable = ["\\.class-budget$", "\\.domain-"]

[profiles.agent-edit]
enable = ["\\.naming$", "^refs\\.direct-layer-boundary$"]
disable = ["^java\\.class\\.slow-report$"]
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

So:

| Profile field | Meaning |
| ------------- | ------- |
| no `enable`, no `disable` | keep every loaded rule |
| only `enable` | keep only matching rule ids |
| only `disable` | keep every rule except matching ids |
| both | keep matching `enable` ids, then remove matching `disable` ids |

`disable` wins when a rule id matches both lists.

Profiles are selected only by the command line or by a generated harness:

```sh
code-moniker check . --profile agent-edit
code-moniker harness claude . --profile agent-edit --scope src
```

Defining `[profiles.agent-edit]` in TOML does not activate it by itself.
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
| `{name.snake}` | bare def name converted from PascalCase/camelCase to snake_case |
| `{kind}` | def kind |
| `{moniker}` | full def moniker |
| `{expr}` | raw expression |
| `{value}` | failing left-hand value |
| `{expected}` | right-hand literal |
| `{pattern}` `{lines}` `{limit}` `{count}` | aliases for `{expected}` or `{value}` |

Ref-scoped rules also render templates:

| Token | Value |
| ----- | ----- |
| `{kind}` | ref kind |
| `{source.name}` `{source.kind}` `{source.shape}` `{source.moniker}` | source def fields |
| `{target.name}` `{target.kind}` `{target.shape}` `{target.moniker}` | target moniker fields |
| `{atom}` `{actual}` `{expected}` | failing atom and values |

## Output

Text output prints one violation per line:

```text
src/widget.ts:L12-L18 [ts.class.name-pascalcase] class `lower_bad` fails `name =~ ^[A-Z][A-Za-z0-9]*$` (name = lower_bad, expected ^[A-Z][A-Za-z0-9]*$)
```

If a custom `message` is present, it is printed as the explanation below
the violation. `rationale` is intentionally omitted from `check` output:

```text
src/widget.ts:L12-L18 [ts.class.name-pascalcase] class `lower_bad` fails `name =~ ^[A-Z][A-Za-z0-9]*$` (name = lower_bad, expected ^[A-Z][A-Za-z0-9]*$)
  → Class names must be PascalCase. Rename `lower_bad`.
```

Project scans end with a summary:

```text
4 violation(s) across 2 file(s) (42 scanned, elapsed 18 ms, 3 error violation(s), 1 warning(s), 1 file(s) errored).
Failed rules:
- ts.class.name-pascalcase: 2 violation(s)
- refs.domain-no-infra: 1 violation(s)
- ts.class.repository-lives-in-domain: 1 warning(s)
Read errors: 1 file(s).
```

`Failed rules` and `Read errors` are printed only when present. The failed
rule list counts unsuppressed violations, so suppressions are already
reflected in the summary.

### JSON

Use JSON when a hook or CI job needs to compute its own summary:

```sh
code-moniker check src/ --format json
code-moniker check src/ --format json --report
```

The top-level shape is:

```json
{
  "summary": {
    "files_scanned": 2,
    "files_with_violations": 1,
    "total_violations": 3,
    "total_rule_errors": 2,
    "total_warnings": 1,
    "files_with_errors": 1,
    "total_errors": 1,
    "elapsed_ms": 18,
    "failed_rules": [
      {
        "rule_id": "ts.class.name-pascalcase",
        "severity": "error",
        "violations": 2
      },
      {
        "rule_id": "refs.domain-no-infra",
        "severity": "error",
        "violations": 1
      }
    ]
  },
  "files": [
    {
      "file": "src/widget.ts",
      "violations": [
        {
          "rule_id": "ts.class.name-pascalcase",
          "severity": "error",
          "moniker": "code+moniker://./lang:ts/module:widget/class:lower_bad",
          "kind": "class",
          "lines": [12, 18],
          "message": "class `lower_bad` fails `name =~ ^[A-Z][A-Za-z0-9]*$`",
          "explanation": "Class names must be PascalCase. Rename `lower_bad`."
        }
      ]
    }
  ],
  "errors": [
    {
      "file": "src/unreadable.ts",
      "error": "cannot read src/unreadable.ts: permission denied"
    }
  ],
  "rule_report": [
    {
      "rule_id": "refs.domain-no-infra",
      "severity": "error",
      "domain": "refs",
      "evaluated": 42,
      "matches": 42,
      "violations": 0,
      "antecedent_matches": 0,
      "warning": "antecedent never matched"
    }
  ]
}
```

`summary` and `files` are always present for supported inputs. `files`
contains every scanned source file, including clean files with an empty
`violations` array. `summary.elapsed_ms` is the wall-clock runtime of the
`check` command in milliseconds. `summary.failed_rules` is sorted by
descending violation count, then by rule id. `errors` is present only when
project mode could not read one or more files. `rule_report` is present
only with `--report` and is omitted when empty.

Violation fields:

| Field | Meaning |
| ----- | ------- |
| `rule_id` | full rule id, such as `ts.class.name-pascalcase` or `refs.domain-no-infra` |
| `severity` | `error` or `warn`; warning violations do not fail `check` by themselves |
| `moniker` | full moniker of the failing def or ref source |
| `kind` | failing def kind, or ref kind for ref-scoped rules |
| `lines` | `[start, end]`, 1-indexed inclusive line range |
| `message` | primary diagnostic text |
| `explanation` | optional custom rule message, when configured |

Rule report fields:

| Field | Meaning |
| ----- | ------- |
| `rule_id` | full rule id |
| `severity` | `error` or `warn` |
| `domain` | evaluated domain, such as `class`, `method`, or `refs` |
| `evaluated` | number of defs or refs considered by the rule |
| `matches` | number of evaluations where the assertion passed |
| `violations` | number of unsuppressed violations |
| `antecedent_matches` | optional count for implication rules (`A => B`) |
| `warning` | optional report warning, for example when an antecedent never matched |

### JSON summaries with `jq`

Print the built-in summary:

```sh
code-moniker check src/ --format json | jq '.summary'
```

List files that have violations:

```sh
code-moniker check src/ --format json \
  | jq -r '.files[] | select(.violations | length > 0) | "\(.file)\t\(.violations | length)"'
```

Count violations by rule:

```sh
code-moniker check src/ --format json \
  | jq -r '[.files[].violations[]]
           | group_by(.rule_id)[]
           | "\(length)\t\(.[0].rule_id)"'
```

Print compiler-style diagnostics:

```sh
code-moniker check src/ --format json \
  | jq -r '.files[] as $file
           | $file.violations[]
           | "\($file.file):\(.lines[0]): [\(.rule_id)] \(.message)"'
```

Show the top rules with one sample location:

```sh
code-moniker check src/ --format json \
  | jq -r '[.files[] as $file
            | $file.violations[]
            | {rule_id, file: $file.file, lines}]
           | group_by(.rule_id)[]
           | "\(length)\t\(.[0].rule_id)\t\(.[0].file):L\(.[0].lines[0])-\(.[0].lines[1])"'
```

Find rules whose implication antecedent never matched:

```sh
code-moniker check src/ --format json --report \
  | jq -r '.rule_report[]?
           | select(.antecedent_matches == 0)
           | "\(.rule_id)\t\(.domain)\tantecedent never matched"'
```

## Next

- Inspect graphs with [`extract`](extract.md).
- Write exact expressions with the [Rule DSL](check-dsl.md).
- Wire `check` into hooks or CI with the [agent harness](agent-harness.md).
