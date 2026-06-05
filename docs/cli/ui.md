# `code-moniker ui`

`code-moniker ui` opens a read-only terminal explorer over the same graph
used by `extract`, `stats`, and `check`. It is meant for quick architecture
inspection on large repositories without leaving the terminal.

```sh
code-moniker ui .
code-moniker ui . --cache .code-moniker-cache
code-moniker ui src --rules .code-moniker.toml --profile agent
code-moniker ui .
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

For startup profiling, set `CODE_MONIKER_UI_LOG` to a TSV output path
before launching the UI:

```sh
CODE_MONIKER_UI_LOG=/tmp/code-moniker-ui.log code-moniker ui .
```

The trace records wall-clock timings for background tasks, workspace
snapshot phases, UI model refreshes, terminal draws, and linkage score
details. A value of `1` writes to `/tmp/code-moniker-ui.log`.
The linkage score is `resolved / (resolved + unresolved + blocked)`;
refs classified as external are reported but do not lower the score.

## MCP Endpoint

MCP runs as a standalone command, without the terminal UI dependency graph:

```sh
cargo run -p code-moniker --features mcp --no-default-features -- mcp . --port 3210
```

The endpoint is `http://127.0.0.1:<port>/mcp`. It exposes compact LMNAV
text responses rather than JSON dumps:

- `code_moniker_read`: workspace discovery. At `workspace` it returns file
  totals, language distribution, concentration by path prefix, language kind
  hints, a paged explorer tree, and follow-up calls. When called with an exact
  symbol URI returned by `code_moniker_symbols`, it reads the source slice for
  that symbol with optional `context_lines`.
- `code_moniker_symbols`: paged symbol rows. It accepts `path`, `lang`,
  `kind`, `shape`, `name`, `limit`, and `cursor` so agents can narrow the
  read before loading broad symbol output. Use `action = "insights"` for
  symbolic metrics: kind/shape distribution, navigable symbol count, and files
  concentrated by symbol or reference volume.
- `code_moniker_rules`: project rule domain. Use `action = "list"` to inspect
  compiled rules, messages, severities, and rationales for the workspace
  languages. Use `action = "run"` to execute `code-moniker check` from the UI
  workspace, optionally with `profile`, `file`, `rules`, and `limit`.

Use `path` globs relative to the UI root, for example
`crates/cli/src/mcp/**`. `limit` caps output rows and `cursor` resumes from
the next page returned in the `next:` section.

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
| `8`, `m` | open the notes lens |
| `n` | edit the selected note, or open a note draft for the selected navigator row |
| `N` | open a new note draft for the selected navigator row |
| `y` | copy a text snapshot of the active right panel to the clipboard |
| `x` | clear the filter |
| `c` | run the check view |
| `?` | show key help |
| `Esc`, left | close navigation or clear the active filtered scope |
| `q`, `Ctrl-C` | quit |

In note mode, `Up`/`Down` and `Tab`/`Shift+Tab` move between kind, title, and
body. `Left`/`Right` change the active kind selector. In title and body fields,
arrow keys, `Home`, and `End` move inside the active editor. `Enter` inserts a
body line break, `Ctrl+s` saves, `Ctrl+d` deletes after a second confirmation
press, `Ctrl+o`/`Ctrl+p` move status through allowed transitions, and `Esc`
closes the editor. Closing an empty new draft creates nothing; closing an
existing note never deletes it.

Notes attach to the selected navigator row. Declarations and files keep their
normal symbol or source URI. Structural rows such as workspace, language,
directory, and change grouping rows use a stable
`code+moniker://workspace/navigation/...` URI.

The UI does not modify source or rules files. It can create or update
project notes in `.code-moniker/notes.toml`. When `--cache DIR` is used,
it may create or update cache entries under that directory; without
`--cache`, it reads project sources and the selected rules file, plus the
project notes file when present.
Panel snapshots are copied as plain text with the active component marker,
mode, scope, and panel content lines so they can be pasted into an issue
or an agent conversation. They are text-oriented debug payloads, not
terminal image captures.

Right-side panels use a shared presentation layer: sections, key/value
details, tables, lists, muted hints, and separators are styled consistently
from the UI theme. Keep new panel content on those helpers so snapshots and
terminal rendering stay readable in the same way.
