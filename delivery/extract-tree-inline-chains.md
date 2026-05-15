# Extract Tree Inline Chains

## Goal

Make `code-moniker extract --format tree` easier to scan on large Java
and workspace trees by avoiding long vertical chains when each node has
only one child.

## Delivered Behavior

- File tree paths collapse linear directory chains inline, for example
  `src/main/java/Foo.java`.
- Outline namespaces collapse linear `package` chains inline, for
  example `package org.apache.bookkeeper`.
- Semantic outline nodes such as `module`, `class`, `enum`, and `method`
  remain separate rows to preserve source ranges and structure.
- Existing color and ASCII/UTF-8 glyph options continue to apply.

## Example

```bash
code-moniker extract . --kind enum --format tree
```

## Validation

- Unit coverage in `crates/cli/src/format/tree.rs` for file path
  collapsing.
- Unit coverage in `crates/cli/src/format/tree.rs` for Java package
  chain collapsing.
