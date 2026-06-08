# Code Moniker Rule Notebook (VSCode)

A workbench for **learning, authoring, and testing
[code-moniker](../README.md) check rules** — understand a rule, edit it, run it,
and see the violations highlighted, for every supported language.

It has three parts, all backed by the `code-moniker` CLI:

- **Notebooks** (`.cmnb`) — runnable lessons; sample cells run checks against
  the rule cells below, while rule cells validate real `.code-moniker.toml`
  fragments.
- **Code Moniker panel** (activity bar) — a **Rule Files** view that manages a
  project's `.code-moniker.toml` / `*.fragment.toml`, and a **Catalog** view of
  the DSL based on the docs.
- **Icons** for notebooks and rule files.

## The Code Moniker panel

Open it from the activity bar (the ◆ icon).

**Rule Files** lists every `.code-moniker.toml` and `*.fragment.toml` in the
workspace, expandable to the rules inside (scope, id, severity). Per file:

- **Validate** — compiles the rules and reports errors in the Problems panel.
- **Run on Project** — runs `code-moniker check` and shows every violation as a
  diagnostic on the real call sites (Problems panel + inline squiggles).

Per rule:

- **Test in Notebook** — opens a scratch notebook with that exact rule and an
  editable sample, so you can probe it interactively.
- Clicking a rule reveals it in its file.

**Catalog** is a curated, runnable reference and a personal sample library.
Builtin entries open as stable scratch notebooks named after the catalog entry:
you can edit the sample or rules, close without saving the shipped file, reopen
the same entry, and **Reset** it to the original example. The tree can be viewed
by learning path, language, or rule, and filtered by source, language, or text.

**User catalog** entries are regular workspace `.cmnb` files under
`.code-moniker/catalog` by default. Copy a builtin entry to the user catalog from
the context menu, drag it onto the **User catalog** group, or drag it from the
Catalog to the Explorer as a `.cmnb` file. The copy becomes a normal editable
notebook that is saved with the project.

## Icons

`.cmnb`, `.code-moniker.toml`, and `*.fragment.toml` get icons in the Code
Moniker panel automatically. For the file Explorer, enable the bundled file icon
theme: **Preferences: File Icon Theme → Code Moniker** (or keep your current
theme — language icons are provided as a best-effort fallback).

## What a notebook contains

A `.cmnb` notebook mixes three kinds of cells:

| Cell        | What it is                                              |
|-------------|--------------------------------------------------------|
| **Markdown**| Pedagogical prose explaining the rule.                 |
| **Sample**  | A code snippet (any supported language) to test against.|
| **Rule**    | One DSL expression. Run it (▷) to validate TOML.       |

When you run a **sample cell**, the extension evaluates the following rule cells
of the same language until the next sample cell. A symbol *violates* a rule when
the expression is `false` for it. The output renders the sample with the
violating lines highlighted, and the same violations are also surfaced as
diagnostics on the sample code cell.

When you run a **rule cell**, the extension only validates that the TOML fragment
compiles as a rule set.

Notebook cells show their role in the cell status bar:

- sample cells show their language and how many following rules they check;
- rule cells show their language and validation role;
- rule cells expose **Copy to config**, which appends the rule fragment to the
  workspace `.code-moniker.toml` without the lesson-only `default_rules` line.

## Requirements

- The `code-moniker` CLI on your `PATH` (or set `codeMoniker.binaryPath`).
  Install from the workspace root:

  ```sh
  cargo install --path crates/cli
  ```

## Getting started

1. Build the extension:

   ```sh
   cd vscode-extension
   npm install
   npm run compile
   ```

2. Press **F5** in VSCode to launch an Extension Development Host.
3. Open a shipped lesson from the **Catalog** view, or run **Code Moniker: New
   Rule Notebook** from the command palette.
4. Run a sample cell (▷) to check the code against the rules below it; run a
   rule cell to validate the rule TOML.

## Rule cells

A rule cell is a **real `.code-moniker.toml` fragment** — exactly what you paste
into a project. It can hold one or more rule blocks, plus `[aliases]`,
`[profiles]`, and `default_rules`:

```toml
default_rules = false

[[rust.fn.where]]
id        = "snake-case"
expr      = "name =~ ^[a-z][a-z0-9_]*$"
severity  = "warn"
message   = "Function `{name}` should be snake_case."
rationale = "Rust API guidelines: free functions use snake_case."
```

Only one thing lives in cell metadata: **language** — the sample language the
fragment is evaluated against (`rust`, `typescript`, …). The rule cell is
authored in the `cmrule-toml` language, which gives TOML highlighting plus
check-DSL highlighting inside each `expr = "…"`.

A symbol *violates* a rule when its `expr` is `false` for that symbol. Run the
sample cell above a rule to render the compiled rules, the sample with violating
lines highlighted, and diagnostics directly on the code cell.

For the full DSL reference see [`docs/cli/check-dsl.md`](../docs/cli/check-dsl.md),
and copy real-world rule sets from
[`docs/cli/check-samples/`](../docs/cli/check-samples/).

## Commands

- **Code Moniker: Open Learning Notebook** — open one of the shipped guided lessons.
- **Code Moniker: New Rule Notebook** — scaffold a starter notebook for a language.
- **Code Moniker: Catalog View** — switch the Catalog tree between path, language, and rule views.
- **Code Moniker: Filter Catalog** / **Clear Catalog Filters** — scope the Catalog by source, language, or text.
- **Code Moniker: Sort Catalog** — sort catalog entries by title, level, or source.
- **Code Moniker: Reset Catalog Sample** — restore a builtin catalog notebook to its shipped content.
- **Code Moniker: Copy to User Catalog** — copy a builtin sample to the workspace user catalog.
- **Code Moniker: Add Sample Cell** / **Add Rule Cell** — insert cells below the selection.
- **Code Moniker: Copy Rule to Project Config** — append the selected rule cell to `.code-moniker.toml`.
- **Code Moniker: Validate Rules** / **Run on Project** — on a rule file (also in the panel).
- **Code Moniker: Test Rule in Notebook** — on a rule (also in the panel).
- **Code Moniker: Refresh** — refresh the Rule Files or Catalog view.

## Settings

- `codeMoniker.binaryPath` — path to the `code-moniker` binary
  (default `code-moniker`, falling back to `~/.cargo/bin/code-moniker`).
- `codeMoniker.catalog.userFolder` — workspace-relative folder for editable
  user catalog notebooks (default `.code-moniker/catalog`).

## Development

```sh
npm run typecheck   # tsc --noEmit
npm run validate    # structural check of shipped notebooks
npm test            # typecheck + validate
npm run compile     # bundle extension + renderer into dist/
npm run watch       # rebuild on change
```

## How it works

```
rule cell (a .code-moniker.toml fragment, metadata {language})
      ▲  validates by itself with `rules show`
      │
sample cell (metadata {language})
      │  following rule cells below, same language, until next sample
      ▼
rules TOML → temp file;  code-moniker rules eval --rules <tmp> --lang <tag> --format json   (sample on stdin)
      │  JSON { rules, violations }
      ▼
violations renderer + diagnostics  → compiled rules + sample with violating lines highlighted
```

- `src/extension.ts` — activation root; delegates to feature registrars.
- `src/notebook/` — `.cmnb` serializer, kernel, cell commands, status bar, samples, and notebook factories.
- `src/cli/` — binary discovery plus `rules eval`, `check`, validate, and `rules learn` wrappers.
- `src/shared/` — language registry and workspace URI helpers.
- `src/diagnostics/` — VSCode diagnostic mapping for CLI violations.
- `renderer/violations.ts` — the highlighted output view.
- `src/rules/parse.ts` + `src/rules/manager.ts` — the Rule Files view, validation, and diagnostics.
- `src/catalog/data.ts` + `src/catalog/repository.ts` + `src/catalog/catalogView.ts` — the Catalog, builtin/user samples, and scratch notebook store.
- `icons/` — SVG assets and the opt-in file icon theme.
