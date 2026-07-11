# Explore — understand a codebase symbolically

Recipes for reading structure. Every command here was run for real; sample
outputs are abbreviated but faithful.

## Contents

1. First contact with an unknown repo
2. Drill the identity tree
3. Find a symbol
4. Ego view: who calls it, what it calls
5. Trace consumers (usages)
6. Read one file's structure
7. When something misbehaves

## 1. First contact

```sh
code-moniker stats .
```

Instant census (no daemon, ~200 ms on 250 files): file/def/ref counts per
language and per shape. Read it as: which languages dominate, how much
executable surface (`callable`), how ref-dense the code is.

```text
files 246 / defs 14432 / refs 42961
lang ts files 229 defs 13900 …   lang java files 17 defs 532 …
shape callable 2598 / type 541 / namespace 246
```

Then start the daemon if not running:

```sh
code-moniker daemon status .    # health + supported verbs; "stale: rescan required" = restart it
code-moniker daemon start --live-refresh auto . &
```

## 2. Drill the identity tree

The identity tree is the purely symbolic hierarchy (no filesystem noise):
`srcset:* / lang:* / dir:* / package:* / module:* / class:* / fn:*`.

```sh
code-moniker query 'identity.children prefix:""'          # roots: lang:ts (6485 defs), srcset:test…
code-moniker query 'identity.children prefix:"lang:ts/dir:apps"'
```

Each child carries kind, name, aggregate def count, and — when the child is
itself a definition — its full URI. Drill where the defs are.

To see a level **as a graph** (nodes + rolled-up reference edges), use
`identity.graph` — that is the coupling map, covered in `diagnose.md`.

## 3. Find a symbol

```sh
code-moniker query 'symbol.search name:"ChangeService" limit:10'
code-moniker query 'symbol.search name:"change" shape:callable limit:10'
code-moniker query 'symbol.search path:"src/server/**" shape:type limit:20'
```

- `name:` is a substring match on the symbol name.
- `shape:` families: `namespace`, `type`, `callable`, `value`, `annotation`
  (`code-moniker shapes` prints the vocabulary; `code-moniker langs <tag>`
  the per-language kind mapping). Single value, no brackets.
- Every hit line ends with the exact URI — copy it for the next calls.

## 4. Ego view of a unit

```sh
code-moniker query 'symbol.graph focus:"<URI or workspace-relative file path>"'
```

The focus defines a boundary (a function; a class = it + members; a file =
all its symbols). Output partitions every resolved reference:

```text
focus: interface ChangeService (apps/trust/src/server/changes/changeService.ts)
members: 6  internal edges: 0  unresolved refs: 5
< function createChangeService(…) x1 [uses_type]      # callers (outside-in)
< field changeService (server/container.ts) x1 …
> interface ChangeView (changes/change.ts) x4 [uses_type]   # callees (inside-out)
```

Chain it from search in one shot:

```sh
uri=$(code-moniker query 'symbol.search name:"ChangeService" shape:type limit:1' | grep -o 'code+moniker://[^ ]*' | head -1)
code-moniker query "symbol.graph focus:\"$uri\""
```

A file path works as focus too, but only a path that exists — take it from a
search hit, don't reconstruct it.

## 5. Trace consumers

```sh
code-moniker query 'symbol.usages uri:"<exact URI>" limit:20'
```

Incoming usages by default, each with reference kind (`calls`,
`instantiates`, `imports_symbol`, `uses_type`…) and location. A class picks
up its members' traffic; a leaf function shows exact call sites.

Interpretation shortcuts: consumers spread across many directories = shared
contract, handle with care; consumers all in `__tests__` = dead-ish code or
test fixture; zero usages on a `pub`/exported symbol = entry point or dead
export — check both ways before deleting.

## 6. Read one file's structure

```sh
code-moniker extract . --path src/server/container.ts --shape callable --limit 80
code-moniker extract . --path src/lib/mod.rs --format json --max-symbols 40
```

Symbol-level table of contents of a file: kinds, names, visibility, nesting.
Cheaper than reading the file when you only need its shape. Always anchor on
`.` and filter with `--path`.

For quantitative JSON analysis over many files, pass `--all`: the default
`--limit 1000` caps emitted monikers silently (check `emitted_refs` against
`stats` if in doubt).

## 7. When something misbehaves

- `workspace is stale` → `code-moniker daemon stop .` then
  `code-moniker daemon start --live-refresh auto . &`.
- `no daemon` errors → `code-moniker daemon list` shows the registry; every
  workspace root registers its own daemon, and an old daemon may predate a
  query verb while reporting the same version (`daemon status` lists the
  verbs it actually supports).
- `symbol not found` → you guessed the URI. Search first, paste exactly.
- Query returns everything despite a filter → wrong field name; the search
  filter is `name:`, not `text:`. See `query-dsl.md`.
