# `code-moniker extract`

Extract a moniker graph from a file or a directory.

```
code-moniker extract <PATH> [--where '<op> <uri>']... [--kind <name>]...
                            [--name <regex>]... [--shape <shape>]...
                            [--format text|tsv|json|tree] [--count] [--quiet]
                            [--limit <N>|--max-symbols <N>]
                            [--after <MONIKER_URI>] [--all]
                            [--moniker-format compact|uri]
                            [--color auto|always|never] [--charset utf8|ascii]
                            [--with-text] [--path <GLOB>]...
                            [--scheme <SCHEME>] [--project <NAME>] [--cache <DIR>]
```

| `<PATH>`  | Output                                  |
| --------- | --------------------------------------- |
| file      | matched graph records for that file     |
| directory | matched graph records grouped by file   |

The walker honors `.gitignore`. `--scheme` overrides the `code+moniker://` URI prefix (matches the PG GUC `code_moniker.scheme`). `--project <NAME>` sets the project component of every emitted moniker (default `.`); the cache shards by anchor hash, so caches keyed at different projects coexist on disk without collision.

| Extension                                | Language |
| ---------------------------------------- | -------- |
| `.ts` `.tsx` `.js` `.jsx` `.mjs` `.cjs`  | `ts`     |
| `.rs`                                    | `rs`     |
| `.java`                                  | `java`   |
| `.py` `.pyi`                             | `python` |
| `.go`                                    | `go`     |
| `.cs`                                    | `cs`     |
| `.sql` `.plpgsql`                        | `sql`    |

Unknown extension exits `2`.

## Filters

| Flag                  | Op   | Semantics                                              |
| --------------------- | ---- | ------------------------------------------------------ |
| `--where '= <uri>'`   | `=`  | moniker equals                                         |
| `--where '< <uri>'`   | `<`  | byte-lex less than (`<=`, `>`, `>=` analogous)         |
| `--where '@> <uri>'`  | `@>` | element is ancestor of `<uri>`                         |
| `--where '<@ <uri>'`  | `<@` | element is descendant of `<uri>`                       |
| `--where '?= <uri>'`  | `?=` | asymmetric `bind_match`                                |
| `--kind <name>`       | —    | concrete kind (e.g. `class`, `fn`, `calls`)            |
| `--name <regex>`      | —    | regex on the last moniker segment name                 |
| `--shape <shape>`     | —    | kind family (`namespace`, `type`, `callable`, …)       |
| `--path <glob>`       | —    | directory mode file glob, relative to `<PATH>`         |

`--where` is repeatable, AND-combined. `--kind` and `--shape` are repeatable or comma-separated (OR within); `--name` is repeatable but is not comma-separated because commas are valid inside regex quantifiers. These filters are AND-combined with each other. `--name` uses Rust regex syntax and matches the last moniker segment name; callable signatures are matched on their bare name. A `def` matches when its moniker satisfies every predicate; a `ref` matches when its **target** does.

`--path` is repeatable and OR-combined. It filters files before extraction
without changing the root used to build monikers. Use it when you need the
same moniker context as a repo-scoped `check` but only want to inspect a
subtree:

```bash
code-moniker extract . --path 'crates/cli/src/mcp/**' --format json --max-symbols 80
```

Discover valid kinds with `code-moniker langs <TAG>`; the shape vocabulary lives in `code-moniker shapes`. Unknown `--kind` exits `2` with the valid set; unknown `--shape` is rejected at parse time by clap.

## Output formats

### Text (default)

One moniker per line. The `txt` alias is also accepted.

```
<moniker>
<moniker>
```

Text output uses compact monikers by default, for example
`java:app.user/UserService.class:UserService`. Use
`--moniker-format uri` to emit full URIs such as
`code+moniker://./lang:java/package:app/package:user/module:UserService/class:UserService`.

Compact text output is colorized automatically when stdout is a terminal.
Use `--color never` to disable color, or `-c` / `--color always` to force it.
Explicit `--color always|never` wins over terminal environment variables;
`auto` honors `NO_COLOR`, `CLICOLOR_FORCE`, `TERM=dumb`, and `CLICOLOR=0`.

### TSV

```
def<TAB>moniker<TAB>kind<TAB>start..end<TAB>L<a>-L<b><TAB>visibility<TAB>signature<TAB>origin
ref<TAB>target_moniker<TAB>kind<TAB>start..end<TAB>L<a>-L<b><TAB>source=<MONIKER><TAB>alias<TAB>confidence<TAB>receiver_hint
```

`start..end` is byte range (0-indexed); `L<a>-L<b>` is the inclusive line range (1-indexed). Empty fields render as `-`.
TSV is a typed record stream: the first column discriminates the `def` and
`ref` schemas rather than promising one uniform table.

TSV uses compact monikers by default, for example
`java:app.user/UserService.class:UserService`. Use
`--moniker-format uri` to restore full URIs such as
`code+moniker://./lang:java/package:app/package:user/module:UserService/class:UserService`.

### JSON

CLI-specific shape: `{uri, lang, matches: {defs, refs}}`. Not the same shape as `code_graph_to_spec(graph)`; not directly consumable by `code_graph_declare(jsonb)`.

```json
{
  "uri":  "file:///abs/path/to/file.ts",
  "lang": "ts",
  "matches": {
    "defs": [
      { "moniker": "...", "kind": "class", "position": [142, 187], "lines": [12, 18],
        "visibility": "public", "binding": "export", "origin": "extracted" }
    ],
    "refs": [
      { "source": "...", "target": "...", "kind": "extends",
        "position": [220, 243], "lines": [25, 25],
        "binding": "local", "confidence": "name_match" }
    ]
  }
}
```

Empty attributes are omitted (not `null`). `source` on a ref is self-contained — consumers don't need to re-extract to resolve it.

When output is truncated, JSON adds top-level `next_cursor` and `remaining`
fields. Resume with `--after '<next_cursor>'`, or pass `--all` to bypass the
cap.

### Tree (`--format tree`)

Human-readable outline built from moniker segments, defs ordered by source position. Refs render under their source def. Available only when the binary is built with `--features pretty` (the default for `cargo install code-moniker` and release builds).

Linear filesystem and namespace branches are collapsed inline. For example,
`src/main/java` and `package org.apache.bookkeeper` render as single
branches when each segment has only one child.

`--color auto|always|never` controls ANSI; `--charset utf8|ascii` controls glyphs.
Explicit color flags use the same precedence as text output.

**Default filter.** Without `--kind`, the tree drops noise defs (`local`, `param`, `comment`) and hides refs — the output is a structural skeleton. Passing any `--kind` re-enables full output: noise defs and refs both render if they match the filter.

### `--count`, `--quiet`

`--count` prints a single integer to stdout. `--quiet` writes nothing — read the exit code. Mutually exclusive.

### `--limit`, `--after`, `--all`

Default output is capped at 1000 matched graph records to keep agent probes
bounded. `--limit <N>` changes the cap; `--max-symbols <N>` is an explicit
alias for the same cap. `--all` disables it. `--after <MONIKER_URI>` resumes
after the moniker cursor returned by the previous page.
Treat returned cursors as opaque; refs with repeated target monikers may use an
internal `cursor:` segment so pagination can remain strict without skipping
duplicates.

For non-JSON formats, a truncated page writes a stderr notice:

```
code-moniker: ... N more results, use --after '<uri>' or --all
```

`--count` and `--quiet` are not paginated.

## Directory mode

Directory mode uses the same match semantics as single-file mode. Text emits
one moniker per line across all files. TSV prefixes each row with the relative
file path. JSON uses `{emitted_files, emitted_defs, emitted_refs, files}`,
where each file entry has `{file, lang, matches}`. Tree renders a filesystem
tree and then the moniker outline under each file.

Use [`stats`](stats.md) for per-file counts, top kinds, and extraction metrics.

## `--with-text`

Comment defs carry only position. Pass `--with-text` to re-read the source and project the text into a trailing text field on TSV comment `def` rows, or a `text` field in JSON. No-op on non-comment defs.

## `--cache <DIR>`

Opt-in on-disk cache of extracted graphs, also via `CODE_MONIKER_CACHE_DIR`. Keyed on `(absolute path, mtime, size, anchor hash, context hash)`. Mtime/size, anchor, or extraction context changes invalidate the entry. Disabled by default.

Layout:
```
<DIR>/v{LAYOUT_VERSION}_{CACHE_FORMAT_VERSION}/<path-hash[0..2]>/<path-hash>_<anchor-hash>_<context-hash>.bin
```

Body bytes are byte-identical to what the PG extension stores in a `code_graph` Datum. Typical 4× speedup on agent edit/hook cycles over a warm cache.

## Ordering

Deterministic for diffing across runs: `defs` sorted by moniker bytes; `refs` sorted by `(source bytes, target bytes, position)`.

## Exit codes

| Code | Meaning                                                                  |
| ---- | ------------------------------------------------------------------------ |
| `0`  | At least one match (or extraction succeeded with content, no predicate). |
| `1`  | No element satisfied the predicates.                                     |
| `2`  | Usage error: bad path, unknown extension, malformed URI, unknown `--kind`. |

## Examples

```bash
# Full graph as JSON, ready for code_graph_declare
code-moniker extract src/widget.ts --format json > widget.spec.json

# All callables descending from a class (shape filter + ancestry predicate)
code-moniker extract src/widget.ts --shape callable \
  --where '<@ code+moniker://./lang:ts/dir:src/module:widget/class:UserService'

# Cross-file bind_match for a typed method family
code-moniker extract src/ --where '?= code+moniker://./lang:ts/dir:src/module:widget/class:UserService/method:findById(_)'

# Java interfaces ending with Resolver, rendered as an outline
code-moniker extract . --kind interface --name 'Resolver$' --format tree

# TODOs across a tree
code-moniker extract src --kind comment --with-text | grep -i todo

# Per-file extraction metrics
code-moniker stats src
```

## See also

- [Check](check.md) — project-wide scan with a rule DSL.
- [Discovery](langs.md) — `langs` and `shapes` commands.
- [Postgres usage](../postgres/usage.md) — round-trip JSON into Postgres.
