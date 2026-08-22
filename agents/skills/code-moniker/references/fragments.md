# Fragments — rules, views, and which URI to use

A `code-moniker.fragment.toml` is a local architecture file. One file feeds
two loaders. Do not treat its `fragment` field, a view `id`, a rule `id`, or a
symbol selector as the same kind of URI.

## Read before inventing an URI

```text
code_moniker_read uri:"workspace/views"
```

Follow only a **returned** `workspace/views/<view.id>` call. Never build that
path from the fragment name, the file path, or a compact moniker.

If no view is relevant, continue from the user's scope. Do not invent a view
URI.

## One file, three identities

```toml
fragment = "workspace-crate"              # rule namespace only

[[refs.where]]
id = "language-domains-are-private"       # local rule id

[[views]]
id = "workspace-linkage-semantic"         # view URI leaf
scope = "."                               # relative to this file's directory
```

| Name | Where it lives | What it becomes |
|---|---|---|
| Fragment id | `fragment = "…"` | Namespace for rules and aliases. Not an URI. |
| View id | `[[views]] id = "…"` | MCP/query URI `workspace/views/<view.id>` |
| Local rule id | `id = "…"` in the fragment | Check/`rules show` id `refs.<fragment>.<id>` (domain prefix depends on the table) |

`fragment` and view `id` may match (`cli-mcp`) or differ (`workspace-crate` vs
`workspace-linkage-semantic`). Always take the view URI from the listing.

Discovery: only descendants of the canonical `.code-moniker.toml`. A
`--rules other.toml` file does not load sibling fragments.

## Symbol selectors are not monikers

View `symbols = […]` entries are **suffixes of indexed identity**, resolved
inside the view scope (the fragment directory, plus optional `scope`):

```toml
symbols = [
  "module:model/struct:ViewSpec",
  "module:config/fn:load",
]
```

The resolver keeps files under that directory, matches
`identity.contains(selector)`, prefers an exact suffix, skips locals/params
unless asked, and returns at most a few evidence rows.

Do **not** put compact monikers (`rs:crates/…`) or canonical
`code+moniker://…` URIs here unless you copied a full identity from the index.
Missing evidence is coverage, not proof that the declared boundary is absent.

## Rule ids in views vs check

In the fragment and in `[[views]] rules = […]`, use the **local** id
(`language-domains-are-private`).

In `check`, hooks, and `rules show`, the effective id is namespaced
(`refs.workspace-crate.language-domains-are-private`). Views resolve a local
id by unique suffix. Duplicate local ids across fragments make that evidence
`missing`.

Local aliases (`[aliases] panels = "…"` in fragment `ui`) become `ui_panels`
in the effective config. `$panels` in that fragment is rewritten. Shared
aliases belong in `.code-moniker.toml`.

## Writing a fragment

Keep `fragment` and every `id` as a simple token: ASCII letters, digits, `_`
or `-` (aliases: letters, digits, `_` only). Every fragment rule needs an
`id`. Fragments cannot override an existing rule id and cannot declare
`workspace.source_group`.

Executable shape: `code-moniker rules learn fragments`. Rule-id merge details:
`docs/cli/check-dsl.md` (Configuration topology).
