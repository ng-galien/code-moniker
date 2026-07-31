# Source groups and source sets

`workspace.source_group` describes one connected source space. It also maps
non-standard source roots onto code-moniker's existing `srcset` identity.
There is no separate source-set configuration.

This structural mapping belongs only to the workspace root
`.code-moniker.toml`. Rule fragments, standalone rule files, and inline rule
overlays reject `workspace.source_group`; accepting it there would create a
rules-only configuration that scanning and linkage cannot observe. The owner
is checked by path identity against the root being analyzed, not by basename:
`check project --rules other/.code-moniker.toml` rejects structural mappings
from `other` instead of accepting configuration that `project` cannot use.

## Default behavior

Without configuration, extraction keeps its standard path conventions. In
particular, `src/main/**` produces `srcset:main` and `src/test/**` or
`src/tests/**` produces `srcset:test`.

The Java linkage strategy already lets `test` read `main`, while `main` cannot
read `test`. Declared mappings feed that same identity and linkage behavior;
they do not create a second classification layer.

## Declared groups

The original connectivity-only form remains valid:

```toml
[[workspace.source_group]]
roots = ["library", "application"]

[[workspace.source_group]]
roots = ["isolated-tool"]
```

Roots in one table belong to the same connected group. Roots declared in
different tables are isolated when a linkage candidate crosses the group
boundary. Files outside every declared root retain manifest-derived behavior.

For a non-standard layout, map each root to the existing source-set identity:

```toml
[[workspace.source_group]]
roots = [
  { path = "src/java", srcset = "main" },
  { path = "test", srcset = "test" },
]
```

Both roots remain in one connected group. Production symbols are indexed under
`srcset:main`, test symbols under `srcset:test`, and Java applies its existing
`test -> main` visibility rule. Queries, inventories, checks, resolution audits,
and identity graphs therefore see the same source-set distinction.

## Validation and refresh

- roots are relative to the workspace and cannot escape it with `..`;
- roots and mapped `srcset` values must be non-empty;
- a root cannot be declared twice;
- nested mappings are allowed inside one group, with the most specific root
  winning;
- roots from different groups cannot overlap.

Invalid declarations fail source-catalog construction with the configuration
path and the mapping error. Changing `.code-moniker.toml` triggers a full
workspace rescan so source identities, extraction cache entries, linkage, and
incremental file classification stay aligned.
