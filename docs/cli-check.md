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

| Code | Meaning                                                     |
| ---- | ----------------------------------------------------------- |
| `0`  | No violations.                                              |
| `1`  | At least one violation (stdout carries the report).         |
| `2`  | Usage / parse error (bad path, malformed user TOML, etc.).  |

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
