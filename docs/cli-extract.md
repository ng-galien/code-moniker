# `code-moniker` — probe a file or a tree

The bare `code-moniker <path>` form runs the same extractor as the
PostgreSQL extension on local source, without a running PG instance.
The argument shape selects the behavior:

- `<file>` → full graph for that file (the original probe).
- `<dir>` with no filter → per-file summary across the tree.
- `<dir>` with `--kind` or `--where` → filtered defs/refs across the tree.

For the `check` subcommand (project linter / agent guardrail), see
[`cli-check.md`](cli-check.md).

## Synopsis

```
code-moniker <path> [--where '<op> <uri>']... [--kind <name>]...
             [--format tsv|json] [--count] [--quiet] [--with-text]
             [--scheme <SCHEME>]
code-moniker --help
code-moniker --version
```

`<path>` is a file or a directory. For a single file, the language is
dispatched from the extension. For a directory, the walker respects
`.gitignore` (same semantics as `check`) and processes every file with
a known extension in parallel. `--scheme` overrides the default
`code+moniker://` URI prefix (matches the Postgres GUC
`code_moniker.scheme`).

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

`--kind <name>` is validated against the union of the per-language
structural kinds (`LangExtractor::ALLOWED_KINDS`), the internal kinds
every extractor emits (`module`, `comment`, `local`, `param`), and the
cross-language ref kinds (`calls`, `imports_module`, `extends`, …). For
a single file the union is restricted to that file's language; for a
directory it's the union over every language present in the scan.
Unknown kinds exit with code `2` and list the valid set — e.g.
`--kind fn` against a TS tree errors out instead of silently matching
nothing (Rust uses `fn`; TypeScript uses `function`/`method`).

Without any predicate, a single file dumps the full graph; a
directory dumps the per-file summary (see below). Predicate URIs
must match the full anchor produced by the extractor (the `lang:`
segment plus the path encoding for the source file). To discover
the anchor for a file, run the binary once without `--where` and
copy a moniker from the output.

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
ref<TAB>target_moniker<TAB>ref_kind<TAB>start..end<TAB>L<a>-L<b><TAB>source=<URI><TAB>alias<TAB>confidence<TAB>receiver_hint
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
        "source":     "code+moniker://./lang:ts/dir:src/module:widget/class:Foo",
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
inclusive), where `end_line` is the line of the last byte. `source` is the
moniker URI of the def that emitted the ref — self-contained so a consumer
can resolve it without re-extracting, even when the filter excluded the
source from `defs`.

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

## Directory mode

When `<path>` is a directory, the cost of the output scales with what
you ask for:

- No filter → **summary**. One row per scanned file with totals and
  the top kinds. Tiny — readable in full.
- `--kind <k>` or `--where ...` → **filtered list**. Same per-match
  output shape as single-file mode, prefixed by the file path.

The walker uses `ignore::WalkBuilder` (same engine as `check`), so
`.gitignore` / `.ignore` are honored. Files whose extension isn't
in the dispatch table above are skipped silently.

### Summary

`--format tsv` (default), one row per file:

```
<file><TAB><lang><TAB><defs><TAB><refs><TAB><kind:count, …>
```

The trailing column shows the top three def kinds by count, sorted
desc then alphabetically. `--format json` adds the full breakdown:

```json
{
  "total_files": 88,
  "total_defs":  5339,
  "total_refs":  21232,
  "files": [
    {
      "file":        "core/code_graph.rs",
      "lang":        "rs",
      "defs":        109,
      "refs":        462,
      "by_def_kind": { "fn": 4, "method": 23, "struct": 6, ... },
      "by_ref_kind": { "calls": 21, "method_call": 91, ... }
    }
  ]
}
```

### Filtered list

`--format tsv` prefixes every match line with the file path:

```
<file><TAB>def<TAB><moniker><TAB><kind><TAB>…
<file><TAB>ref<TAB><target><TAB><kind><TAB>…
```

`--format json` groups matches per file with the same `matches` shape
as single-file output:

```json
{
  "total_files": 12,
  "total_defs":  37,
  "total_refs":  0,
  "files": [
    { "file": "src/cli/dir.rs", "lang": "rs", "matches": { "defs": [...], "refs": [] } }
  ]
}
```

`--count` returns the total across all files; `--quiet` exits `0` when
at least one file matched.

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

| Code | Meaning                                                                                |
| ---- | -------------------------------------------------------------------------------------- |
| `0`  | At least one match (or, without predicates, extraction succeeded with content).        |
| `1`  | Extraction succeeded but no element satisfied the predicates.                          |
| `2`  | Usage error: bad path, unknown extension, malformed URI, or unknown `--kind` for lang. |

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

# Per-file summary across the whole src tree (small, readable)
code-moniker src

# All function defs across the tree (filtered list)
code-moniker src --kind fn

# Refs to a specific symbol across the tree
code-moniker src --where '?= code+moniker://./lang:rs/module:walk/fn:walk_lang_files'
```

## See also

- [`cli-check.md`](cli-check.md) — project-wide scan with a rule DSL.
- [`use-in-postgres.md`](use-in-postgres.md) — round-trip the JSON
  output into Postgres via `code_graph_declare`.
