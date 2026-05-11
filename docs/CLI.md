# `code-moniker` CLI

A standalone binary that runs the same extractors and the same predicates as the
PostgreSQL extension, but on a single source file, without requiring a running
PostgreSQL instance. Same `core::moniker` and `core::code_graph` types behind
the scenes; only the I/O changes (stdin/stdout instead of SQL).

## Synopsis

```
code-moniker <file> [--where '<op> <uri>']... [--kind <name>]...
             [--format tsv|json] [--count] [--quiet] [--with-text]
code-moniker check <file> [--rules <path>] [--format text|json]
code-moniker --help
code-moniker --version
```

`<file>` is a path to a source file. Language is dispatched from the extension:

| Extension                | Language tag |
| ------------------------ | ------------ |
| `.ts` `.tsx` `.js` `.jsx`| `ts`         |
| `.rs`                    | `rs`         |
| `.java`                  | `java`       |
| `.py`                    | `python`     |
| `.go`                    | `go`         |
| `.cs`                    | `cs`         |
| `.sql` `.plpgsql`        | `sql` (only when built with `--features pg17`) |

Unknown extension exits with code `2`.

## Predicates

A single flag `--where '<op> <uri>'` exposes the eight moniker operators
shared with the SQL extension. The URI uses the canonical typed form
(`<lang>+moniker://<project>/<kind>:<name>...`). Single quotes around the
predicate avoid shell I/O redirection on `<` and `>`.

| `--where 'op uri'`       | SQL operator | Semantics                                          |
| ------------------------ | ------------ | -------------------------------------------------- |
| `--where '= <uri>'`      | `=`          | Moniker equals.                                    |
| `--where '< <uri>'`      | `<`          | Byte-lex strictly less than.                       |
| `--where '<= <uri>'`     | `<=`         | Byte-lex less or equal.                            |
| `--where '> <uri>'`      | `>`          | Byte-lex strictly greater than.                    |
| `--where '>= <uri>'`     | `>=`         | Byte-lex greater or equal.                         |
| `--where '@> <uri>'`     | `@>`         | The element is an ancestor of `<uri>`.             |
| `--where '<@ <uri>'`     | `<@`         | The element is a descendant of `<uri>`.            |
| `--where '?= <uri>'`     | `?=`         | Asymmetric `bind_match` (per-language arms).       |
| `--kind <name>`          | —            | Element kind equals `<name>` (repeatable, OR).     |

`--where` is repeatable; predicates compose via implicit `AND`. A `def`
matches when its own moniker satisfies every predicate; a `ref` matches when
its **target** moniker satisfies every predicate.

Without any predicate, the binary dumps the full graph.

```sh
# Methods of class Foo
code-moniker file.ts --where '<@ ts+moniker://./class:Foo' --kind method

# Anything that resolves to Foo (cross-file bind)
code-moniker file.ts --where '?= ts+moniker://./class:Foo'

# Exact handle of a typed callable
code-moniker file.ts --where '= ts+moniker://./fn:handle(string)→void'
```

## Output

### `--format tsv` (default)

One match per line, tab-separated. The first column is `def` or `ref`.

For a `def`:

```
def<TAB>moniker<TAB>kind<TAB>start..end<TAB>L<a>-L<b><TAB>visibility<TAB>signature<TAB>origin
```

Two position columns. `start..end` is the raw byte range matching
`core::Position` (0-indexed); `L<a>-L<b>` is the corresponding 1-indexed
inclusive line range — `end_line` is the line containing the last byte, so a
slice ending right after a newline does not reach into the next line. Empty
fields are rendered as `-`.

For a `ref`:

```
ref<TAB>target_moniker<TAB>ref_kind<TAB>start..end<TAB>L<a>-L<b><TAB>source_idx=N<TAB>alias<TAB>confidence<TAB>receiver_hint
```

The `ref_kind` is one of the canonical lowercase tokens from `core::kinds`:
`calls`, `method_call`, `extends`, `implements`, `uses_type`, `imports`,
`imports_module`, `reexports`, `instantiates`, `reads`, `annotates`, etc.

### `--format json`

A single JSON document on stdout, intentionally identical in shape to
`code_graph_to_spec(graph)`. Round-trippable into Postgres via
`code_graph_declare(jsonb)`.

```json
{
  "uri":  "file:///abs/path/to/file.ts",
  "lang": "ts",
  "matches": {
    "defs": [
      {
        "moniker":    "ts+moniker://./lang:ts/module:widget/class:Foo",
        "kind":       "class",
        "position":   [142, 187],
        "lines":      [12, 18],
        "visibility": "public",
        "origin":     "extracted"
      }
    ],
    "refs": [
      {
        "source_idx": 4,
        "target":     "ts+moniker://./lang:ts/module:widget/class:Foo",
        "kind":       "extends",
        "position":   [220, 243],
        "lines":      [25, 25],
        "confidence": "name_match"
      }
    ]
  }
}
```

Attribute fields (`visibility`, `signature`, `binding`, `origin`, `alias`,
`confidence`, `receiver_hint`, `text`) are omitted when empty rather than
rendered as `null`. `position` is `[start_byte, end_byte]` (0-indexed),
matching `core::Position`. `lines` is `[start_line, end_line]` (1-indexed,
inclusive), where `end_line` is the line of the last byte. `source_idx`
indexes into the same `defs` array.

### `--count`

Suppresses the per-match output and prints a single integer (the number of
matches) on stdout. Mutually exclusive with `--quiet`.

### `--quiet`

Suppresses output entirely. Combine with the exit code to write shell guards:

```bash
if code-moniker file.ts --kind comment --quiet; then
    echo "file has at least one comment"
fi
```

## Comments

Each AST comment node yields a def of `kind: comment`, with a moniker scoped to
the lexical context (`ts+moniker://./class:Foo/comment:<start_byte>` for a
comment inside class `Foo`). The kind name is the same across all seven
extractors. The disambiguator is the comment's start byte, so distinct comments
never collide.

The `code_graph` does not store comment text — only the position. To project
the text, pass `--with-text`: the binary re-reads the source file at each
comment's position and adds:

- TSV: a final column with the raw text, `\t` and `\n` escaped.
- JSON: a `"text"` string field on every comment def.

`--with-text` is a no-op for non-comment defs.

```bash
# Are there any comments?
code-moniker file.ts --kind comment --quiet
# Count them
code-moniker file.ts --kind comment --count
# Find TODOs in the file
code-moniker file.ts --kind comment --with-text --format tsv | grep -i todo
# Comments inside a specific class
code-moniker file.ts --where '<@ ts+moniker://./class:Foo' --kind comment
```

## Stable ordering

Output ordering is deterministic so it can be diffed across runs:

- `defs` sorted by moniker bytes (the canonical byte-lex order of the type).
- `refs` sorted by `(source moniker bytes, target moniker bytes, position)`.
- `position` is byte-range; `lines` is the same range projected to 1-indexed
  inclusive line numbers — both are always present alongside each other.

## Exit codes

| Code | Meaning                                                                          |
| ---- | -------------------------------------------------------------------------------- |
| `0`  | At least one match (or, without predicates, extraction succeeded with content).  |
| `1`  | Extraction succeeded but no element satisfied the predicates.                    |
| `2`  | Usage error: bad file path, unknown extension, malformed URI in a predicate.     |

`--quiet` and `--count` follow the same exit semantics — they only affect what
is written to stdout.

## Examples

```bash
# Full graph as JSON, ready for code_graph_declare
code-moniker src/widget.ts --format json > widget.spec.json

# All methods of class Foo
code-moniker src/widget.ts --where '<@ ts+moniker://./class:Foo' --kind method

# Does file define a function called `handle` taking a string?
code-moniker src/server.ts --where '= ts+moniker://./function:handle(string)→_' --quiet

# All references to a specific symbol's family (bind_match)
code-moniker src/widget.ts --where '?= ts+moniker://./class:Foo/method:bar(_)→_'

# Count comments and exit 1 if there are none
code-moniker file.py --kind comment --count
```

## Non-goals

- **No multi-file ingestion.** One file per invocation; combine via shell.
- **No persistence.** Output is text; round-tripping is what `code_graph_declare`
  is for inside Postgres.
- **No DSL.** Predicates are flags. Repeated `--kind` is the only OR; everything
  else is AND.

## `check` — live linter for agent harnesses

```
code-moniker check <path> [--rules <path>] [--format text|json]
```

`<path>` is either a single source file (per-edit lint) or a directory
(project-wide scan). Loads an embedded default rule pack, optionally merges
a user `<path>` (default `.code-moniker.toml`) on top, and reports
violations on stdout.

Per-file mode is designed for `PostToolUse` hooks; project mode walks the
tree respecting `.gitignore` / `.ignore` / hidden-file rules (via the
`ignore` crate) and processes recognised extensions in parallel
(`rayon`). The output shape is the same in both modes — a single file
just produces a one-entry `files` list.

Exit codes:

| Code | Meaning                                                     |
| ---- | ----------------------------------------------------------- |
| `0`  | No violations.                                              |
| `1`  | At least one violation (stdout carries the report).         |
| `2`  | Usage / parse error (bad path, malformed user TOML, etc.).  |

### Configuration

Full DSL reference: **[docs/CHECK_DSL.md](CHECK_DSL.md)**. Grammar, scopes
(`[[<lang>.<kind>.where]]` for defs, `[[refs.where]]` for refs),
quantifiers (`any` / `all` / `none` / `count` on `<kind>` / `segment` /
`out_refs` / `in_refs`), path patterns (`moniker ~ '**/class:/Port$/'`),
aliases (`$name`), and a worked example covering Clean Code, DDD, Hex and
bounded-context invariants live there.

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

### Custom messages

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

### Suppressions

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

### Output

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
          "moniker": "ts+moniker://./lang:ts/module:widget/class:lower_bad",
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

### Hook recipe — Claude Code `PostToolUse`

Drop in `.claude/settings.json` at the repo root:

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

Exit `1` is surfaced to the agent as feedback in the conversation, which
lets it self-correct before continuing. Exit `2` (e.g. unsupported file
extension) is silent for the agent — the hook is a no-op for non-source
edits.
