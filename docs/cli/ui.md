# `code-moniker ui`

`code-moniker ui` opens a read-only terminal explorer over the same graph
used by `extract`, `stats`, and `check`. It is meant for quick architecture
inspection on large repositories without leaving the terminal.

```sh
code-moniker ui .
code-moniker ui . --cache .code-moniker-cache
code-moniker ui src --rules .code-moniker.toml --profile architecture
```

## What It Loads

The UI scans supported source files, extracts defs and refs, then builds
an in-memory index for navigation:

- declarations by moniker, kind, and name;
- parent/child outline links;
- incoming and outgoing refs;
- extraction metrics by language, shape, and kind.

Use `--cache DIR` for repeated scans. The cache format is shared with
`stats` and stores extracted `CodeGraph` records keyed by source metadata
and moniker anchor.

## Views

| View | Purpose |
| ---- | ------- |
| `overview` | file/def/ref totals, timings in ms, language and shape distributions |
| `outline` | selected declaration, children, source excerpt |
| `refs` | incoming and outgoing references for the selected declaration |
| `check` | runs `.code-moniker.toml` rules against the loaded graph |

## Keys

| Key | Action |
| --- | ------ |
| `Tab`, `1`-`4` | switch views |
| `j`/`k`, arrows | move selection |
| `/` | filter declaration names with a Rust regex |
| `x` | clear the filter |
| `c` | run the check view |
| `?` | show key help |
| `q`, `Esc`, `Ctrl-C` | quit |

The UI does not modify source or rules files. When `--cache DIR` is used,
it may create or update cache entries under that directory; without
`--cache`, it only reads project sources and the selected rules file.
