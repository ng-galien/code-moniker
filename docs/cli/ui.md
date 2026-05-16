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
| `change` | Git-backed `HEAD..worktree` symbols and blast-radius usages |

## Modes

The header reports the current UI mode and scope:

- `mode explorer  scope all` for normal navigation;
- `mode search  scope /Resolver` for structural filters;
- `mode search  scope search:customer` for ranked symbol search;
- `mode usages  scope MoneyFormatter` when focusing references to a symbol;
- `mode change  scope HEAD..worktree` for Git-local changed symbols.

Ranked symbol search uses a dedicated `ui.search.input` field above
`ui.navigator`. The field is focused while editing and remains visible
after applying the search so the navigator context is explicit.

Change mode uses Git when the loaded source root is inside a repository.
In single-source and multi-source sessions, roots without Git are reported
in `ui.panel.change` instead of failing the UI. In explorer mode, changed
declarations keep `+` or `~` badges with direct usage counts. In change
mode, `ui.navigator` shows added, modified, and removed declarations;
`u` toggles the selected change between diff details and blast-radius
references.

The UI keeps its store live with filesystem watchers. Source changes
reload the in-memory index; `.git` changes refresh only the change index.
Generated cache and build paths such as `.code-moniker-cache/`, `target/`,
`build/`, `dist/`, `.gradle/`, and `node_modules/` are ignored. Custom
`--cache DIR` paths are also ignored when they live inside a watched root.

## Keys

| Key | Action |
| --- | ------ |
| `Tab`, `1`-`5` | switch views |
| `j`/`k`, arrows | move selection |
| `/` | filter declaration names with a Rust regex |
| `s` | search declarations with ranked symbol search |
| `d` | toggle Git change mode (`HEAD..worktree`) |
| `u` | focus usages; in change mode, toggle blast-radius details |
| `y` | copy a text snapshot of the active right panel to the clipboard |
| `x` | clear the filter |
| `c` | run the check view |
| `?` | show key help |
| `Esc`, left | close navigation or clear the active filtered scope |
| `q`, `Ctrl-C` | quit |

The UI does not modify source or rules files. When `--cache DIR` is used,
it may create or update cache entries under that directory; without
`--cache`, it only reads project sources and the selected rules file.
Panel snapshots are copied as plain text with the active component marker,
mode, scope, and panel content lines so they can be pasted into an issue
or an agent conversation. They are text-oriented debug payloads, not
terminal image captures.
