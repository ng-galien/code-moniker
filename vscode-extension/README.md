# Code Moniker VSCode Extension (Beta)

A beta VSCode workbench for [code-moniker](../README.md) check rules: browse
rule files, validate them, run them on a workspace, and open executable
learning and sample scenarios from the repository's Markdown scenario corpus.
It is an extension surface for the CLI, not a claim that every supported source
language has the same extractor maturity.

For source installation, user settings, and packaged `.vsix` instructions, see
the dedicated [VS Code extension guide](../docs/vscode-extension.md).

## The Code Moniker Panel

Open it from the activity bar.

**Rule Files** lists every `.code-moniker.toml` and `*.fragment.toml` in the
workspace, expandable to the rules inside. Per file:

- **Validate** compiles the rules and reports errors in the Problems panel.
- **Run on Project** runs `code-moniker check` and shows violations as
  diagnostics on real project files.
- Clicking a rule reveals it in its source file.

**Catalog** is built from [`samples/learn/`](../samples/learn/) and
[`samples/catalog/`](../samples/catalog/) and bundled into the extension.
Opening an entry creates an editable clean clone of that `.cm.md` scenario
notebook, such as `basics.cm.md` or `rust-naming.cm.md`. The bundled scenario is
never modified; if the user changes rules or code, VSCode marks the notebook
dirty and regular Save / Save As chooses where to persist the `.cm.md` file.

The catalog tree can be viewed by learning path, language, or rule, and filtered
by language or text.

## Icons

`.code-moniker.toml` and `*.fragment.toml` get rule icons in the Code Moniker
panel automatically. For the file Explorer, enable the bundled file icon theme:
**Preferences: File Icon Theme -> Code Moniker**.

## Scenario Samples

Learn and catalog samples are Markdown documents with fenced blocks tagged by
the scenario DSL:

````markdown
```toml cm:rules
default_rules = false

[[rust.fn.where]]
id = "snake-case"
expr = "name =~ ^[a-z][a-z0-9_]*$"
```

```rust cm:file=src/lib.rs
fn BadName() {}
```

```cm:expect
snake-case
```
````

The extension opens those documents with the `code-moniker-scenario` notebook
view so each scenario can be executed, while the persisted source remains plain
Markdown.

In the notebook UI, running a `cm:file` cell checks that scenario file with the
scenario rules. Running a `cm:rules` cell, or running multiple cells, checks the
whole in-memory scenario workspace. Bundled scenarios do not need to be saved
before running: the extension sends the current notebook content to
`code-moniker check --scenario -`. `cm:expect` is kept for CLI scenario tests and
is ignored by the interactive notebook runner.

For the DSL reference see [`docs/cli/check-dsl.md`](../docs/cli/check-dsl.md).
`code-moniker rules learn` prints the same executable learning topics from
[`samples/learn/`](../samples/learn/).

## Commands

- **Code Moniker: Open Catalog Sample** — open a bundled learn or sample scenario.
- **Code Moniker: Open Sample Scenario** — pick a builtin scenario directly.
- **Code Moniker: Catalog View** — switch the Catalog tree between path,
  language, and rule views.
- **Code Moniker: Filter Catalog** / **Clear Catalog Filters** — scope the
  Catalog by language or text.
- **Code Moniker: Sort Catalog** — sort catalog entries by title or level.
- **Code Moniker: Validate Rules** / **Run on Project** — operate on a rule
  file from the Rule Files view.
- **Code Moniker: Refresh** — refresh the Rule Files or Catalog view.
- **Code Moniker: Connect Workspace Daemon** / **Refresh Daemons** — manage the
  daemon session used by workspace features.
- **Code Moniker: Refresh Symbols** — reload the daemon-backed symbol outline.
- **Code Moniker: Run Check** — run the daemon-backed workspace check.

## Settings

- `codeMoniker.binaryPath` — explicit path to a `code-moniker` binary. This
  overrides the bundled binary in a platform-specific VSIX and is intended for
  development or troubleshooting.
- `codeMoniker.daemon.autoConnect` — connect to or start the workspace daemon
  when a folder opens.

## Development

Install the CLI first when running the extension against a local checkout:

```sh
cargo install --path ../crates/cli --features tui,mcp
```

```sh
npm ci
npm run typecheck   # tsc --noEmit
npm run validate    # catalog sample import/shape checks
npm test            # typecheck + validate
npm run compile     # bundle extension + renderer into dist/
npm run watch       # rebuild on change
```

Package and install the extension from this directory:

```sh
npm run package
code --install-extension code-moniker-0.1.0.vsix
```

## Structure

- `src/extension.ts` — activation root; delegates to feature registrars.
- `src/scenario/` — Markdown scenario notebook serializer, execution controller,
  and opener.
- `src/catalog/` — catalog tree, commands, repository, and sample imports.
- `src/cli/` — binary discovery plus `rules eval`, `check`, validate, and
  scenario execution wrappers.
- `src/daemon/` — daemon discovery, startup, JSON-RPC session, and tree nodes.
- `src/symbols/` — daemon-backed symbol outline and detail panel.
- `src/rules-daemon/` — daemon-backed check execution and diagnostics.
- `src/rules/` — Rule Files view, parser, validation, and diagnostics.
- `src/shared/` — language registry and workspace URI helpers.
- `src/diagnostics/` — VSCode diagnostic mapping for CLI violations.
- `renderer/violations.ts` — highlighted scenario output renderer.
- `icons/` — SVG assets and the opt-in file icon theme.
