# `pg-moniker` CLI

A standalone binary that runs the same extractors and the same predicates as the
PostgreSQL extension, but on a single source file, without requiring a running
PostgreSQL instance. Same `core::moniker` and `core::code_graph` types behind
the scenes; only the I/O changes (stdin/stdout instead of SQL).

## Synopsis

```
pg-moniker <file> [predicate]... [--kind <name>]... [--format tsv|json]
                  [--count] [--quiet] [--with-text]
pg-moniker check <file> [--rules <path>] [--format text|json]
pg-moniker --help
pg-moniker --version
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

Each predicate flag takes a moniker URI in the same canonical typed form as the
extension (`<lang>+moniker://<project>/<kind>:<name>...`). When the binary is
built with the default scheme, the project authority defaults to `.` for
single-file extraction.

| Flag                       | Semantics                                                                                |
| -------------------------- | ---------------------------------------------------------------------------------------- |
| `--eq <uri>`               | Moniker equals (`=`).                                                                    |
| `--lt <uri>` / `--le <uri>`| Strictly / non-strictly less than (byte-lex `<` / `<=`).                                 |
| `--gt <uri>` / `--ge <uri>`| Strictly / non-strictly greater than.                                                    |
| `--ancestor-of <uri>`      | The element's moniker is an ancestor of `<uri>` (`@>`).                                  |
| `--descendant-of <uri>`    | The element's moniker is a descendant of `<uri>` (`<@`).                                 |
| `--bind <uri>`             | Asymmetric `bind_match` (the `?=` operator), with the same per-language arms.            |
| `--kind <name>`            | Element kind equals `<name>` (e.g. `class`, `method`, `function`, `comment`, `import`).  |

Predicates compose via implicit `AND`. A `def` is a match if its own moniker
satisfies every predicate. A `ref` is a match if its **target** moniker
satisfies every predicate.

A `--kind` flag may be repeated, in which case the kinds are OR-combined. All
other flags are accepted at most once.

Without any predicate, the binary dumps the full graph.

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
if pg-moniker file.ts --kind comment --quiet; then
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
pg-moniker file.ts --kind comment --quiet
# Count them
pg-moniker file.ts --kind comment --count
# Find TODOs in the file
pg-moniker file.ts --kind comment --with-text --format tsv | grep -i todo
# Comments inside a specific class
pg-moniker file.ts --descendant-of 'ts+moniker://./class:Foo' --kind comment
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
pg-moniker src/widget.ts --format json > widget.spec.json

# All methods of class Foo
pg-moniker src/widget.ts --descendant-of 'ts+moniker://./class:Foo' --kind method

# Does file define a function called `handle` taking a string?
pg-moniker src/server.ts --eq 'ts+moniker://./function:handle(string)→_' --quiet

# All references to a specific symbol's family (bind_match)
pg-moniker src/widget.ts --bind 'ts+moniker://./class:Foo/method:bar(_)→_'

# Count comments and exit 1 if there are none
pg-moniker file.py --kind comment --count
```

## Non-goals

- **No multi-file ingestion.** One file per invocation; combine via shell.
- **No persistence.** Output is text; round-tripping is what `code_graph_declare`
  is for inside Postgres.
- **No DSL.** Predicates are flags. Repeated `--kind` is the only OR; everything
  else is AND.

## `check` — live linter for agent harnesses

```
pg-moniker check <file> [--rules <path>] [--format text|json]
```

Loads an embedded default rule pack, optionally merges a user `<path>` (default
`.pg-moniker.toml`) on top, and reports violations on stdout. Designed to be
invoked from a `PostToolUse` hook so an agent gets immediate feedback on each
edit.

### Configuration — TOML

Each section is `[<lang>.<kind>]`. Five rule fields, all optional:

```toml
[ts.class]
name_pattern = "^[A-Z][A-Za-z0-9]*$"

[ts.function]
name_pattern = "^[a-z_][A-Za-z0-9]*$"
max_lines    = 60

[ts.method]
max_count_per_parent = 20            # max methods per class

[ts.comment]
allow_only_patterns = ['^\s*//\s*(TODO|@ts-)', '^\s*/\*\*']  # whitelist mode
forbid_patterns     = ['eslint-disable(?!-next-line)']        # block-form ban
```

Rule field semantics:

| Field                    | Triggered when                                                                              |
| ------------------------ | ------------------------------------------------------------------------------------------- |
| `name_pattern`           | the bare last-segment name does not match the regex.                                        |
| `forbid_name_patterns`   | the name matches ANY of these regex (denylist for placeholder names: `helper`, `utils`, …).|
| `max_lines`              | the def's source span exceeds N physical lines.                                             |
| `max_count_per_parent`   | one parent contains more than N children of this kind.                                      |
| `forbid_patterns`        | a comment text matches any pattern (deny-list).                                             |
| `allow_only_patterns`    | a comment text matches NONE of the patterns (strict allow-list).                            |
| `require_doc_comment`    | def whose visibility matches the configured value lacks a doc comment immediately above it. |

`require_doc_comment` takes a string: a visibility name (`"public"`, `"private"`, `"package"`, …) restricts the rule to defs of that visibility, or `"any"` applies it regardless. A def is considered documented iff a comment def ends before its start byte AND only ASCII whitespace separates the two — so `/** doc */\nfn foo()` passes, `/** doc */ stuff\nfn foo()` doesn't.

The `default` section provides fall-back rules across languages
(`[default.<kind>]` is consulted when a kind has nothing in `[<lang>.<kind>]`).

### Custom messages — `[<lang>.<kind>.messages]`

For an agent harness, a factual diagnostic ("name does not match
`^[A-Z][A-Za-z0-9]*$`") is rarely enough — the agent needs the **rule to
follow**. Each kind can carry an optional `messages` sub-table keyed by rule
name; matching violations get the rendered template attached as
`explanation`, displayed alongside the engine's factual `message` (not in
place of).

```toml
[ts.class]
name_pattern = "^[A-Z][A-Za-z0-9]*$"

[ts.class.messages]
name_pattern = """
Class names must be PascalCase. Rename `{name}` to match `{pattern}`.
See CLAUDE.md §naming.
"""

[ts.function]
max_lines = 60

[ts.function.messages]
max_lines = "Function `{name}` is {lines} lines, max {limit}. Split it."
```

**Placeholders** (substituted by literal `str::replace`):

| Token        | Available in                                                              |
| ------------ | ------------------------------------------------------------------------- |
| `{name}`     | every rule (bare callable name, signature stripped)                       |
| `{kind}`     | every rule                                                                |
| `{moniker}`  | every rule (full URI)                                                     |
| `{pattern}`  | `name_pattern`, `forbid_patterns`                                         |
| `{lines}`    | `max_lines`                                                               |
| `{limit}`    | `max_lines`, `max_count_per_parent`                                       |
| `{count}`    | `max_count_per_parent`                                                    |

Unknown placeholders are left intact. Unknown message keys (rule names that
don't exist) are silently ignored — forward-compatible.

In the text output, the explanation is shown indented under the violation:

```
src/widget.ts:L12-L18 [ts.class.name_pattern] name `lower_bad` does not match `^[A-Z][A-Za-z0-9]*$`
  → Class names must be PascalCase. Rename `lower_bad` to match `^[A-Z][A-Za-z0-9]*$`.
  → See CLAUDE.md §naming.
```

In JSON, it lands as a sibling field of `message`:

```json
{
  "rule_id":     "ts.class.name_pattern",
  "message":     "name `lower_bad` does not match `^[A-Z][A-Za-z0-9]*$`",
  "explanation": "Class names must be PascalCase. Rename `lower_bad` to match `^[A-Z][A-Za-z0-9]*$`.\nSee CLAUDE.md §naming."
}
```

### Suppressions

```ts
// pg-moniker: ignore                            // suppress every rule on the next def
// pg-moniker: ignore[name_pattern]              // only that rule (suffix match)
// pg-moniker: ignore-file                       // whole file
// pg-moniker: ignore-file[max_lines]            // whole file, single rule
```

The directive prefix is the language's line-comment marker (`//`, `#`, `--`).
Rule IDs follow the TOML path — `ts.class.name_pattern`, `python.function.max_lines`,
etc. — and the filter accepts any suffix (`name_pattern` matches every
`<lang>.<kind>.name_pattern`).

### Output

Default text format — one violation per line, similar to ESLint stylish:

```
src/widget.ts:L12-L18 [ts.class.name_pattern] name `lower_bad` does not match `^[A-Z][A-Za-z0-9]*$`
src/widget.ts:L24-L24 [ts.comment.allow_only_patterns] prose comment forbidden — only directives in the allow-list are permitted
```

`--format json` emits a single document with the same fields:

```json
{
  "file": "src/widget.ts",
  "violations": [
    {
      "rule_id": "ts.class.name_pattern",
      "moniker": "ts+moniker://./lang:ts/module:widget/class:lower_bad",
      "kind":    "class",
      "lines":   [12, 18],
      "message": "name `lower_bad` does not match `^[A-Z][A-Za-z0-9]*$`"
    }
  ]
}
```

### Exit codes (`check`)

| Code | Meaning                                                                  |
| ---- | ------------------------------------------------------------------------ |
| `0`  | No violation.                                                            |
| `1`  | At least one violation. Stdout carries the report.                       |
| `2`  | Usage / parse error (bad path, unknown extension, malformed user TOML).  |

The semantic mirrors `pg-moniker file.ts` (0 = match, 1 = no match), so a
shell wrapper using `if pg-moniker check ...` reads naturally as "any
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
            "command": "pg-moniker check \"$CLAUDE_FILE_PATH\""
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
