# `code-moniker check` — project linter

```
code-moniker check <path> [--rules <path>] [--format text|json]
```

The `check` subcommand evaluates a declarative rule pack against the
symbol graph of one file or a whole project. It is the entry point for
agent guardrails, pre-commit hooks, and CI gates. For the per-file
probe form, see [`cli-extract.md`](cli-extract.md). For the full rule
grammar, see [`check-dsl.md`](check-dsl.md). For an end-to-end
integration walkthrough, see [`use-as-agent-harness.md`](use-as-agent-harness.md).

`<path>` is either a single source file (per-edit lint) or a directory
(project-wide scan). Loads an embedded default rule pack, optionally
merges a user `<path>` (default `.code-moniker.toml`) on top, and
reports violations on stdout.

Per-file mode is designed for `PostToolUse` hooks; project mode walks
the tree respecting `.gitignore` / `.ignore` / hidden-file rules (via
the `ignore` crate) and processes recognised extensions in parallel
(`rayon`). The output shape is the same in both modes — a single file
just produces a one-entry `files` list.

Exit codes:

| Code | Meaning                                                                                          |
| ---- | ------------------------------------------------------------------------------------------------ |
| `0`  | No violations. Also returned when the target file's extension isn't a recognised source language — per-file mode is a silent no-op so PostToolUse hooks don't spam the agent on docs / configs. |
| `1`  | At least one violation (stdout carries the report).                                              |
| `2`  | Usage error (bad path, malformed user TOML).                                                     |

## Configuration

Full DSL reference: [`check-dsl.md`](check-dsl.md). Grammar, scopes
(`[[<lang>.<kind>.where]]` for defs, `[[refs.where]]` for refs),
quantifiers (`any` / `all` / `none` / `count` on `<kind>` / `segment` /
`out_refs` / `in_refs`), path patterns (`moniker ~ '**/class:/Port$/'`),
aliases (`$name`), and a worked example covering Clean Code, DDD, Hex
and bounded-context invariants live there.

Minimal shape:

```toml
[[ts.class.where]]
id      = "no-god-class"
expr    = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` is too wide ({value})."

[[refs.where]]
id   = "domain-no-infra"
expr = "source ~ '**/module:domain/**' => NOT target ~ '**/module:infrastructure/**'"

[ts.class]
require_doc_comment = "public"
```

`require_doc_comment` is a separate field on the kind block (not part of
`where`). Value is a visibility name (`"public"`, `"private"`, `"any"`).
A def is documented iff a comment def ends on the line immediately above
the def's **doc anchor** — which is the earliest of (the def's own start,
any `annotates` ref position for this def). That handles
`/** doc */\n@Decorator\nclass Foo` correctly.

## Recipes

The patterns below cover the architectural concerns most projects ask the
linter to enforce: Clean Code (defs stay small and well-named),
Domain-Driven Design (building blocks have a fixed shape), Hexagonal
Architecture (dependencies only point inward), and bounded contexts
(modules talk through a shared contract). Each recipe stands alone — drop
it into `.code-moniker.toml`, adjust the path globs to your layout, and
the rule fires on the next `code-moniker check`.

Path encoding depends on the language. TS / JS / TSX / JSX, Rust, Go and
C# encode directories as `dir:<seg>`; Java and Python encode packages as
`package:<seg>`; PL/pgSQL encodes schemas as `schema:<name>`. Run
`code-moniker <file> --format json` once to see what the extractor
produces and align the globs with that.

The examples assume a TypeScript layout: `src/domain/`,
`src/application/`, `src/infrastructure/`, plus the bounded contexts
`src/billing/`, `src/shipping/`, `src/contract/`. Aliases keep the path
globs readable:

```toml
[aliases]
domain   = "moniker ~ '**/dir:domain/**'"
app      = "moniker ~ '**/dir:application/**'"
infra    = "moniker ~ '**/dir:infrastructure/**'"
contract = "moniker ~ '**/dir:contract/**'"
```

`$name` expands textually before parsing, so the alias bundles its
projection (`moniker ~ '...'`). For ref-scoped rules write `source ~ '...'`
/ `target ~ '...'` explicitly — aliases don't compose under a prefix.

### Clean Code — keep defs small and verb-named

```toml
[[ts.class.where]]
id   = "no-god-class"
expr = "count(method) <= 20 AND count(field) <= 7 AND all(method, lines <= 60)"

[[ts.constructor.where]]
id   = "small-ctor"
expr = "count(param) <= 4"

[[ts.method.where]]
id   = "name-is-a-verb"
expr = "NOT name =~ ^(do|handle|process|manage)[A-Z]"

[[ts.method.where]]
id   = "method-not-named-after-parent"
expr = "name != parent.name"
```

`count(method)` and `count(field)` iterate the direct children defs of
the class under check. `all(method, lines <= 60)` is a quantifier: the
inner expression is evaluated with each method bound as the current
item. The naming rule rejects vague prefixes that paper over
multi-purpose helpers; `parent.name` reads the bare name of the
moniker's penultimate segment, catching `OrderService.OrderService(...)`.

### DDD — shape the building blocks

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
expr = "name =~ Repository$ => $domain"

[[ts.class.where]]
id   = "use-case-shape"
expr = """
  name =~ UseCase$
  => count(method) = 1 AND any(method, name = 'execute')
"""

[[ts.class.where]]
id   = "port-is-never-a-class"
expr = "NOT name =~ Port$"
```

Every rule is gated by a suffix premise (`name =~ Entity$ =>`), so only
defs that opt in by name are evaluated. `=>` is the implication — drop
it and the rule fires on every class that isn't an entity. The
`port-is-never-a-class` rule has no premise: ports are interfaces, full
stop.

### Hexagonal — dependency direction at the boundaries

```toml
[[refs.where]]
id   = "domain-depends-on-nothing-but-itself"
expr = "source ~ '**/dir:domain/**' => target ~ '**/dir:domain/**'"

[[refs.where]]
id   = "application-only-inward"
expr = """
  source ~ '**/dir:application/**'
  => target ~ '**/dir:application/**' OR target ~ '**/dir:domain/**'
"""

[[refs.where]]
id   = "domain-imports-no-framework"
expr = """
  source ~ '**/dir:domain/**' AND kind = 'imports_symbol'
  => NOT (target ~ '**/external_pkg:express/**'
          OR target ~ '**/external_pkg:nestjs/**'
          OR target ~ '**/external_pkg:typeorm/**'
          OR target ~ '**/external_pkg:prisma/**')
"""

[[refs.where]]
id   = "infra-implements-application-ports-only"
expr = """
  source ~ '**/dir:infrastructure/**' AND kind = 'implements'
  => target ~ '**/dir:application/**' AND target.name =~ Port$
"""
```

`[[refs.where]]` is poly-lang: one rule covers every language the
project mixes. `kind = '...'` narrows the ref type — `imports_symbol`
for imports, `implements` for inheritance edges. `target.name` reads the
bare callable name of the moniker's last segment, so the port-suffix
convention is enforced without a path pattern. The framework blacklist
lives in the import rule, not the layering rule, because it speaks
about external packages rather than source layout.

### Bounded contexts — talk only through a shared contract

```toml
[[refs.where]]
id   = "billing-touches-shipping-only-via-contract"
expr = """
  source ~ '**/dir:billing/**' AND target ~ '**/dir:shipping/**'
  => target ~ '**/dir:contract/**'
"""
```

Two contexts (`billing`, `shipping`) may only exchange types defined in
the shared `contract` module. The premise narrows to refs that already
cross the boundary; the consequent demands they land in `contract/`.
Duplicate the pattern per pair of contexts that should stay isolated.

### Adapters and controllers

```toml
[[ts.class.where]]
id   = "thin-controller"
expr = """
  name =~ Controller$
  => count(method) <= 8
     AND count(class) = 0
     AND all(method, lines <= 30)
"""

[[ts.class.where]]
id   = "adapter-implements-a-port"
expr = """
  name =~ Adapter$
  => any(out_refs, kind = 'implements'
                   AND target ~ '**/dir:application/**'
                   AND target.name =~ Port$)
"""

[[ts.class.where]]
id   = "low-fan-out"
expr = """
  kind = 'class'
  => count(out_refs, kind = 'uses_type'
                     AND NOT target ~ '**/external_pkg:/**') <= 7
"""
```

`out_refs` iterates the refs whose source is the current def — useful
when a structural rule asserts something about what the def *does*, not
just what it *contains*. The `adapter-implements-a-port` rule reads as
"every `*Adapter` must have at least one `implements` edge to an
application-layer `*Port`"; the coupling rule counts non-external
`uses_type` edges and caps them at 7.

### Fixture and test code stays in test modules

```toml
[[ts.class.where]]
id   = "fixtures-only-in-test-modules"
expr = """
  name =~ ^(Stub|Mock|Fake|Builder)$
  => any(segment, segment.kind = 'dir'
                  AND segment.name =~ (^tests?$|_test$))
"""
```

`segment` iterates the moniker top-down; the rule walks the def's own
path and demands at least one segment is a `tests/` or `*_test/`
directory. Production code that shadows a fixture name fires.

### Doc comments — spatial, outside `where`

```toml
[ts.class]
require_doc_comment = "public"

[ts.interface]
require_doc_comment = "any"
```

`require_doc_comment` is not an expression — it's a field on the kind
block. Value `"public"` lints public defs only, `"any"` lints them all.

The full grammar (operators, projections, alias scoping) and a single
consolidated `.code-moniker.toml` covering every recipe above live in
[`check-dsl.md`](check-dsl.md). For wiring `check` into a Claude Code
hook, a pre-commit gate, or CI, see
[`use-as-agent-harness.md`](use-as-agent-harness.md).

## Custom messages

Each `where` entry carries an optional `message` template. When the rule
fires, the template is rendered with placeholders:

| Token        | Value                                                |
| ------------ | ---------------------------------------------------- |
| `{name}`     | def's bare callable name                             |
| `{kind}`     | def's kind                                           |
| `{moniker}`  | def's full URI                                       |
| `{expr}`     | the raw expression that fired                        |
| `{value}`    | the actual LHS value                                 |
| `{expected}` | the RHS literal                                      |
| `{pattern}` `{lines}` `{limit}` `{count}` | legacy aliases of `{expected}`/`{value}` for familiar wording |

Unknown placeholders are left intact.

## Suppressions

```ts
// code-moniker: ignore                       // suppress every rule on the next def
// code-moniker: ignore[name-pascalcase]      // only that rule id (suffix match)
// code-moniker: ignore-file                  // whole file
// code-moniker: ignore-file[max-lines]       // whole file, single rule
```

The directive prefix is the language's line-comment marker (`//`, `#`,
`--`). Rule ids follow the TOML path: `<lang>.<kind>.<id>` where `<id>` is
either the explicit `id` on the entry or `where_<index>` if omitted. The
suppression filter matches by suffix.

In the text output, the explanation is shown indented under the violation:

```
src/widget.ts:L12-L18 [ts.class.name-pascalcase] class `lower_bad` fails `name =~ ^[A-Z][A-Za-z0-9]*$`
  → Class names must be PascalCase. Rename `lower_bad`.
```

In JSON, it lands as a sibling field of `message`:

```json
{
  "rule_id":     "ts.class.name-pascalcase",
  "message":     "class `lower_bad` fails `name =~ ^[A-Z][A-Za-z0-9]*$` (name = lower_bad, expected ^[A-Z][A-Za-z0-9]*$)",
  "explanation": "Class names must be PascalCase. Rename `lower_bad`."
}
```

## Output

Default text format — one violation per line, similar to ESLint stylish,
with a trailing summary in project mode:

```
src/widget.ts:L12-L18 [ts.class.name-pascalcase] class `lower_bad` fails `name =~ ^[A-Z][A-Za-z0-9]*$`
src/widget.ts:L24-L24 [ts.function.max-lines]    function `loadEverything` fails `lines <= 60`
src/order.ts:L5-L20  [ts.class.no-god-class]     class `Order` fails `count(method) <= 20`

3 violation(s) across 2 file(s) (42 scanned).
```

`--format json` emits one document with a summary and a `files` array:

```json
{
  "summary": {
    "files_scanned": 42,
    "files_with_violations": 2,
    "total_violations": 3
  },
  "files": [
    {
      "file": "src/widget.ts",
      "violations": [
        {
          "rule_id": "ts.class.name-pascalcase",
          "moniker": "code+moniker://./lang:ts/module:widget/class:lower_bad",
          "kind":    "class",
          "lines":   [12, 18],
          "message": "class `lower_bad` fails `name =~ ^[A-Z][A-Za-z0-9]*$`"
        }
      ]
    }
  ]
}
```

The semantic mirrors `code-moniker file.ts` (0 = match, 1 = no match), so a
shell wrapper using `if code-moniker check ...` reads naturally as "any
problems?".

## Next steps

- Plug into an agent loop or CI gate → [`use-as-agent-harness.md`](use-as-agent-harness.md).
- Write your first rule → [`check-dsl.md`](check-dsl.md).
- Probe a single file ad-hoc → [`cli-extract.md`](cli-extract.md).
