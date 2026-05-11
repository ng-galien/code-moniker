# `code-moniker` — probe a single file

The bare `code-moniker <file>` form runs the same extractor as the
PostgreSQL extension on a single source file, without a running PG
instance.

For the `check` subcommand (project linter / agent guardrail), see
[`cli-check.md`](cli-check.md).

## Synopsis

```
code-moniker <file> [--where '<op> <uri>']... [--kind <name>]...
             [--format tsv|json] [--count] [--quiet] [--with-text]
             [--scheme <SCHEME>]
code-moniker --help
code-moniker --version
```

`<file>` is a path to a source file. Language is dispatched from the
file extension. `--scheme` overrides the default `code+moniker://`
URI prefix (matches the Postgres GUC `code_moniker.scheme`).

| Extension                                 | Language tag |
| ----------------------------------------- | ------------ |
| `.ts` `.tsx` `.js` `.jsx` `.mjs` `.cjs` | `ts`         |
| `.rs`                                     | `rs`         |
| `.java`                                   | `java`       |
| `.py` `.pyi`                              | `python`     |
| `.go`                                     | `go`         |
| `.cs`                                     | `cs`         |
| `.sql` `.plpgsql`                         | `sql`        |

Unknown extension exits with code `2`.

## Predicates

`--where '<op> <uri>'` exposes the moniker operators shared with the
SQL extension. The URI uses the canonical typed form,
`code+moniker://<project>/<kind>:<name>[/...]`. Single quotes avoid
shell I/O redirection on `<` and `>`.

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

`--where` is repeatable; predicates compose via implicit `AND`. A
`def` matches when its own moniker satisfies every predicate; a
`ref` matches when its **target** moniker satisfies every predicate.

Without any predicate, the binary dumps the full graph. Predicate
URIs must match the full anchor produced by the extractor (the
`lang:` segment plus the path encoding for the source file). To
discover the anchor for a file, run the binary once without
`--where` and copy a moniker from the output.

```sh
# All methods inside a class, given the file's full anchor
code-moniker src/widget.ts \
  --where '<@ code+moniker://./lang:ts/dir:src/module:widget/class:UserService' \
  --kind method

# Anything that resolves to UserService (cross-file bind_match)
code-moniker src/widget.ts \
  --where '?= code+moniker://./lang:ts/dir:src/module:widget/class:UserService'

# Exact handle of a typed callable
code-moniker src/widget.ts \
  --where '= code+moniker://./lang:ts/dir:src/module:widget/class:UserService/method:findById(string)'
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

The `ref_kind` is one of the canonical lowercase tokens used by the
extractors: `calls`, `method_call`, `extends`, `implements`,
`uses_type`, `imports_symbol`, `imports_module`, `reexports`,
`instantiates`, `reads`, `annotates`, `di_register`, `di_require`.

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
        "moniker":    "code+moniker://./lang:ts/dir:src/module:widget/class:Foo",
        "kind":       "class",
        "position":   [142, 187],
        "lines":      [12, 18],
        "visibility": "public",
        "binding":    "export",
        "origin":     "extracted"
      }
    ],
    "refs": [
      {
        "source_idx": 0,
        "target":     "code+moniker://./lang:ts/dir:src/module:widget/class:Bar",
        "kind":       "extends",
        "position":   [220, 243],
        "lines":      [25, 25],
        "confidence": "name_match",
        "binding":    "local"
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

Each AST comment node yields a def of `kind: comment`, with a
moniker scoped to the lexical context (a comment inside `class:Foo`
gets `.../class:Foo/comment:<start_byte>`). The kind name is the
same across every extractor. The disambiguator is the comment's
start byte, so distinct comments never collide.

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

# Comments inside a specific class (full anchor required)
code-moniker src/widget.ts --kind comment \
  --where '<@ code+moniker://./lang:ts/dir:src/module:widget/class:UserService'
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

# All methods of a class (anchor includes lang: + path encoding)
code-moniker src/widget.ts --kind method \
  --where '<@ code+moniker://./lang:ts/dir:src/module:widget/class:UserService'

# Does the file define `findById(string)` on UserService?
code-moniker src/widget.ts --quiet \
  --where '= code+moniker://./lang:ts/dir:src/module:widget/class:UserService/method:findById(string)'

# Refs that bind-match a symbol family (signature collapsed per-lang)
code-moniker src/widget.ts \
  --where '?= code+moniker://./lang:ts/dir:src/module:widget/class:UserService/method:findById(_)'

# Count comments and exit 1 if there are none
code-moniker file.py --kind comment --count
```

## See also

- [`cli-check.md`](cli-check.md) — project-wide scan with a rule DSL.
- [`use-in-postgres.md`](use-in-postgres.md) — round-trip the JSON
  output into Postgres via `code_graph_declare`.
