# Code Moniker VSCode Extension

A VSCode workbench for [code-moniker](../README.md) check rules: browse rule
files, validate them, run them on a workspace, and open executable catalog
samples from the repository's Markdown scenario corpus.

## The Code Moniker Panel

Open it from the activity bar.

**Rule Files** lists every `.code-moniker.toml` and `*.fragment.toml` in the
workspace, expandable to the rules inside. Per file:

- **Validate** compiles the rules and reports errors in the Problems panel.
- **Run on Project** runs `code-moniker check` and shows violations as
  diagnostics on real project files.
- Clicking a rule reveals it in its source file.

**Catalog** is built from [`samples/catalog/`](../samples/catalog/) and bundled
into the extension. Opening a catalog entry creates an editable unsaved scenario
notebook from that bundled template, named as an untitled `.cm.md` file such as
`rust-naming.cm.md`. The builtin sample is never modified; if the user changes
rules or code, VSCode marks the notebook dirty and regular Save / Save As
chooses where to persist the `.cm.md` file.

The catalog tree can be viewed by learning path, language, or rule, and filtered
by language or text.

## Icons

`.code-moniker.toml` and `*.fragment.toml` get rule icons in the Code Moniker
panel automatically. For the file Explorer, enable the bundled file icon theme:
**Preferences: File Icon Theme -> Code Moniker**.

## Scenario Samples

Catalog samples are Markdown documents with fenced blocks tagged by the scenario
DSL:

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
whole in-memory scenario workspace. Catalog samples do not need to be saved
before running: the extension sends the current notebook content to
`code-moniker check --scenario -`. `cm:expect` is kept for CLI scenario tests and
is ignored by the interactive notebook runner.

For the DSL reference see [`docs/cli/check-dsl.md`](../docs/cli/check-dsl.md).
For pedagogical CLI learning topics, use `code-moniker rules learn`, backed by
[`samples/learn/`](../samples/learn/).

## Commands

- **Code Moniker: Open Catalog Sample** — open a catalog scenario.
- **Code Moniker: Open Sample Scenario** — pick a builtin scenario directly.
- **Code Moniker: Catalog View** — switch the Catalog tree between path,
  language, and rule views.
- **Code Moniker: Filter Catalog** / **Clear Catalog Filters** — scope the
  Catalog by language or text.
- **Code Moniker: Sort Catalog** — sort catalog entries by title or level.
- **Code Moniker: Validate Rules** / **Run on Project** — operate on a rule
  file from the Rule Files view.
- **Code Moniker: Refresh** — refresh the Rule Files or Catalog view.

## Settings

- `codeMoniker.binaryPath` — path to the `code-moniker` binary
  (default `code-moniker`, falling back to `~/.cargo/bin/code-moniker`).

## Development

```sh
npm run typecheck   # tsc --noEmit
npm run validate    # catalog sample import/shape checks
npm test            # typecheck + validate
npm run compile     # bundle extension + renderer into dist/
npm run watch       # rebuild on change
```

## Structure

- `src/extension.ts` — activation root; delegates to feature registrars.
- `src/scenario/` — Markdown scenario notebook serializer, execution controller,
  and opener.
- `src/catalog/` — catalog tree, commands, repository, and sample imports.
- `src/cli/` — binary discovery plus `rules eval`, `check`, validate, and
  scenario execution wrappers.
- `src/rules/` — Rule Files view, parser, validation, and diagnostics.
- `src/shared/` — language registry and workspace URI helpers.
- `src/diagnostics/` — VSCode diagnostic mapping for CLI violations.
- `renderer/violations.ts` — highlighted scenario output renderer.
- `icons/` — SVG assets and the opt-in file icon theme.
