# `code-moniker extract`

Extract a moniker graph from a file or a directory.

```
code-moniker extract <PATH> [--where '<op> <uri>']... [--kind <name>]...
                            [--name <regex>]... [--shape <shape>]...
                            [--format tsv|json|tree] [--count] [--quiet]
                            [--color auto|always|never] [--charset utf8|ascii]
                            [--with-text] [--scheme <SCHEME>] [--project <NAME>]
                            [--cache <DIR>]
```

| `<PATH>`     | Output                                                  |
| ------------ | ------------------------------------------------------- |
| file         | full graph for that file                                |
| directory    | per-file summary (no filter) or filtered list           |

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

`--where` is repeatable, AND-combined. `--kind` and `--shape` are repeatable or comma-separated (OR within); `--name` is repeatable but is not comma-separated because commas are valid inside regex quantifiers. These filters are AND-combined with each other. `--name` uses Rust regex syntax and matches the last moniker segment name; callable signatures are matched on their bare name. A `def` matches when its moniker satisfies every predicate; a `ref` matches when its **target** does.

Discover valid kinds with `code-moniker langs <TAG>`; the shape vocabulary lives in `code-moniker shapes`. Unknown `--kind` exits `2` with the valid set; unknown `--shape` is rejected at parse time by clap.

## Output formats

### TSV (default)

```
def<TAB>moniker<TAB>kind<TAB>start..end<TAB>L<a>-L<b><TAB>visibility<TAB>signature<TAB>origin
ref<TAB>target_moniker<TAB>kind<TAB>start..end<TAB>L<a>-L<b><TAB>source=<URI><TAB>alias<TAB>confidence<TAB>receiver_hint
```

`start..end` is byte range (0-indexed); `L<a>-L<b>` is the inclusive line range (1-indexed). Empty fields render as `-`.

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

### Tree (`--format tree`)

Human-readable outline built from moniker segments, defs ordered by source position. Refs render under their source def. Available only when the binary is built with `--features pretty` (the default for `cargo install code-moniker` and release builds).

`--color auto|always|never` controls ANSI; `--charset utf8|ascii` controls glyphs.

**Default filter.** Without `--kind`, the tree drops noise defs (`local`, `param`, `comment`) and hides refs — the output is a structural skeleton. Passing any `--kind` re-enables full output: noise defs and refs both render if they match the filter.

### `--count`, `--quiet`

`--count` prints a single integer to stdout. `--quiet` writes nothing — read the exit code. Mutually exclusive.

## Directory mode

Without filters: one row per file with totals + top kinds.

```
<file><TAB><lang><TAB><defs><TAB><refs><TAB><kind:count, …>
```

`--format json` adds full `by_def_kind` / `by_ref_kind` breakdowns.

With `--kind`/`--shape`/`--where`: same match shape as single-file, prefixed by the file path.

## `--with-text`

Comment defs carry only position. Pass `--with-text` to re-read the source and project the text into a `text` column (TSV) or field (JSON). No-op on non-comment defs.

## `--cache <DIR>`

Opt-in on-disk cache of extracted graphs, also via `CODE_MONIKER_CACHE_DIR`. Keyed on `(absolute path, mtime, size, anchor hash)`. Mtime/size change invalidates the entry. Disabled by default.

Layout:
```
<DIR>/v{LAYOUT_VERSION}_{CACHE_FORMAT_VERSION}/<path-hash[0..2]>/<path-hash>_<anchor-hash>.bin
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

# Per-file summary
code-moniker extract src
```

## See also

- [Check](check.md) — project-wide scan with a rule DSL.
- [Discovery](langs.md) — `langs` and `shapes` commands.
- [Postgres usage](../postgres/usage.md) — round-trip JSON into Postgres.
