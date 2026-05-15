# Extract Text Monikers

## Goal

Make the default `code-moniker extract` output easy to scan by emitting
only monikers, while preserving TSV for scripts that need metadata
columns.

## Delivered Behavior

- `--format text` is the default and emits one moniker per line.
- `--format txt` is accepted as an alias for `text`.
- `--format tsv` keeps the tabular rows with kind, position, lines,
  visibility, signature, origin, and ref metadata.
- Text and TSV use compact monikers by default, collapsing language,
  package, module, and semantic segments into a concise form such as
  `java:app.user/UserService.class:UserService`.
- Text output colorizes compact monikers automatically on a TTY.
  `-c` / `--color always` forces color, and `--color never` disables it.
- `--moniker-format uri` restores full `code+moniker://` URI rendering
  for text and TSV output.
- JSON output remains canonical and still emits full URIs.
- Tree output is unchanged; it already renders a compact outline.

## Example

```bash
code-moniker extract src/Foo.java
code-moniker extract src/Foo.java -c
code-moniker extract src/Foo.java --format tsv
code-moniker extract src/Foo.java --format text --moniker-format uri
```

## Validation

- Unit coverage in `crates/cli/src/format.rs` for compact and full URI
  rendering.
- E2E coverage in `crates/cli/tests/cli_e2e.rs` for CLI defaults and
  `--format tsv`, `--format txt`, and `--moniker-format uri`.
