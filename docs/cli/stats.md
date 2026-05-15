# `code-moniker stats`

Report extraction-only metrics for a file or directory.

```
code-moniker stats <PATH> [--format tsv|json|tree]
                         [--color auto|always|never] [--charset utf8|ascii]
                         [--project <NAME>] [--cache <DIR>]
```

`stats` walks the same supported source files as `extract`, honors
`.gitignore`, and emits no moniker rows. Use it to size a repository,
compare extractor coverage, or measure scan overhead before enabling
rules.

## Metrics

| Metric | Meaning |
| ------ | ------- |
| `total_files` | supported source files scanned |
| `total_defs` / `total_refs` | graph records emitted by extractors |
| `by_lang` | file, def, and ref totals per language tag |
| `by_shape` | distribution across `namespace`, `type`, `callable`, `value`, `annotation`, `ref` |
| `by_kind.defs` / `by_kind.refs` | concrete kind histograms |
| `timings.scan_ms` | path walk and language detection time, in milliseconds |
| `timings.extract_ms` | graph extraction time, in milliseconds |
| `timings.total_ms` | command wall-clock time, in milliseconds |

All timing values are integer milliseconds. With `--cache`, warm runs measure
cache lookup and decode time rather than full parser time.

## Examples

```bash
# Compact TSV output
code-moniker stats src

# Machine-readable report for a large repository
code-moniker stats dogfood/ts/date-fns --format json

# Human-readable tree, with colors forced for CI logs or demos
code-moniker stats . --format tree --color always

# Reuse the extraction cache during repeated measurements
CODE_MONIKER_CACHE_DIR=.code-moniker-cache code-moniker stats . --format json
```

## Exit Codes

| Code | Meaning |
| ---- | ------- |
| `0` | at least one supported file was scanned |
| `1` | no supported source file was found |
| `2` | usage error, such as an unreadable path or unsupported single-file extension |
