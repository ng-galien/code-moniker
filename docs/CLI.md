# `pg-moniker` CLI

A standalone binary that runs the same extractors and the same predicates as the
PostgreSQL extension, but on a single source file, without requiring a running
PostgreSQL instance. Same `core::moniker` and `core::code_graph` types behind
the scenes; only the I/O changes (stdin/stdout instead of SQL).

## Synopsis

```
pg-moniker <file> [predicate]... [--kind <name>]... [--format tsv|json]
                  [--count] [--quiet] [--with-text]
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
def<TAB>moniker<TAB>kind<TAB>start..end<TAB>visibility<TAB>signature<TAB>origin
```

The position column is a byte range (`start_byte..end_byte`) into the source
file, matching `core::Position`. Empty fields are rendered as `-`.

For a `ref`:

```
ref<TAB>target_moniker<TAB>ref_kind<TAB>start..end<TAB>source_idx=N<TAB>alias<TAB>confidence<TAB>receiver_hint
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
        "confidence": "name_match"
      }
    ]
  }
}
```

Attribute fields (`visibility`, `signature`, `binding`, `origin`, `alias`,
`confidence`, `receiver_hint`, `text`) are omitted when empty rather than
rendered as `null`. Positions are `[start_byte, end_byte]` into the source
file, matching `core::Position`. `source_idx` indexes into the same `defs`
array.

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
