# pg_code_moniker — specification

PostgreSQL extension. Native types for code symbol identity and code graph storage, with indexed algebra. No table schemas, no triggers, no I/O against external state. Pure types + operators + per-language extractors.

**Implementation language : Rust, via [`pgrx`](https://github.com/pgcentralfoundation/pgrx).** The `core/` modules are pure Rust (no pgrx, no PG deps) and testable in isolation with `cargo test`. The `pg/` modules are thin pgrx wrappers exposing the SQL surface. The `lang/` modules are pure Rust extractors using the `tree-sitter` crate and per-language grammar crates.

## Goals

- Make symbol identity a first-class PostgreSQL type, with O(log n) indexed matching.
- Make a per-module code structure a first-class PostgreSQL type, queryable with operators and indexes.
- Provide per-language extractors that produce code graphs from source via tree-sitter.
- Provide constructors so consumers can build code graphs without source (forward modeling, declared externals).

## Non-goals

- No persistent state owned by the extension.
- No knowledge of any consumer's schema (tables, RLS, triggers, application logic).
- No project-level configuration storage. Callers pass project anchors and presets as function arguments.
- No language whose scoping is not locally determinable.
- No support for cross-project federation.

## Conceptual model

### The canonical tree

A program is a strict tree. Each node has exactly one parent. The root is the project. The path from the root to a node is the node's identity.

The tree has two regimes separated by an event-frontier called the **srcset** :

| Regime | From | To | Depth | Kinds defined by |
|---|---|---|---|---|
| **Project regime** | root | srcset | variable | caller (passed as project preset) |
| **Language regime** | srcset | leaves | variable | language strategy (in extension) |

The srcset is a node at the boundary. Its kind is supplied by the caller. The extension does not interpret srcsets — it receives the srcset moniker as an anchor and emits all source-derived nodes under it.

### The moniker

A node's identity in the canonical tree. Native PostgreSQL type with operators and a custom GiST index.

Canonical external representation is a **typed-segment URI** using a `+moniker` scheme profile:

```
<scheme>+moniker://<project>/<kind>:<name>[/<kind>:<name>...][#<kind>:<name>[#<kind>:<name>...]]
```

The base scheme is caller-configurable (default `pcm`, canonical URI `pcm+moniker://`); a consumer like ESAC can use its own (`esac`, canonical URI `esac+moniker://`). The `+moniker` suffix identifies the canonical typed moniker profile, not the final symbol kind. Kinds are carried by each segment.

Examples below use the default scheme:

```
pcm+moniker://my-app/srcset:main/package:com/package:acme/class:Foo
pcm+moniker://my-app/srcset:main/package:com/package:acme/class:Foo#method:bar()
pcm+moniker://my-app/srcset:main/package:com/package:acme/class:Foo#method:bar(int,String)
pcm+moniker://my-app/srcset:main/dir:src/dir:lib/module:util#class:Helper#method:process()
pcm+moniker://my-app/external_pkg:maven/org.springframework/spring-core/6.1.0
```

The earlier SCIP-like punctuation form remains useful as a compact display form and compatibility input under the base scheme (`pcm://`, `esac://`):

| Punct class | Display shape              | Used for                                     |
|-------------|----------------------------|----------------------------------------------|
| Path        | `…/<name>`                 | srcset, package, directory, file-as-module   |
| Type        | `<name>#`                  | class, interface, enum, type alias           |
| Term        | `<name>.`                  | field, variable, constant                    |
| Method      | `<name>().` / `<name>(N).` | method, function, constructor                |

Display examples:

```
pcm://my-app/main/com/acme/Foo                              file-as-module (Java)
pcm://my-app/main/com/acme/Foo#Foo#                         type Foo
pcm://my-app/main/com/acme/Foo#Foo#bar().                   method bar
pcm://my-app/main/com/acme/Foo#Foo#bar(2).                  overloaded bar, arity 2
pcm://my-app/src/lib/util.ts#Helper#process().              TS method
pcm://my-app/maven/org.springframework/spring-core/6.1.0    external (Maven)
```

The canonical typed `+moniker` form is the durable I/O contract. Compact display form is lossy unless the caller supplies kind defaults, so it must not be the only persisted truth.

The URI is symbolic only — it identifies the node in the tree. It does not encode disk location. Source location remains a sidecar (`source_uri`) on the module row.

Names with reserved characters (`/`, `#`, `:`, `(`, `)`, backtick, whitespace) are wrapped in backticks; literal backticks are doubled.

Detailed URI design and migration rules are documented in `docs/MONIKER_URI.md`.

Operators :

| Op   | Signature                              | Semantics                                          |
|------|----------------------------------------|----------------------------------------------------|
| `=`  | `moniker = moniker → bool`             | Path equality. **The matching primitive.**         |
| `<@` | `moniker <@ moniker → bool`            | Left is descendant of right.                       |
| `@>` | `moniker @> moniker → bool`            | Left is ancestor of right.                         |
| `\|\|` | `moniker \|\| (segment text, kind text) → moniker` | Compose child from parent.            |
| `~`  | `moniker ~ moniker_pattern → bool`     | Pattern match.                                     |

Accessors : `kind_of(moniker) → text`, `project_of(moniker) → text`, `path_of(moniker) → text[]`, `parent_of(moniker) → moniker`, `depth(moniker) → int`.

I/O functions : `moniker_in(cstring) → moniker`, `moniker_out(moniker) → cstring`. URI parsing and serialization are part of the type's I/O contract.

GiST opclass : custom access method indexing `=`, `<@`, `@>`, `~`.

ODR (One Definition Rule) is a property the consumer enforces with `UNIQUE` constraints on columns of type `moniker`. The extension does not police ODR — it provides the type whose equality makes ODR enforceable.

### The code_graph

A native type carrying the internal structure of a single module.

A `code_graph` contains :

- **Tree** : the intra-module containment hierarchy. Root is the module's own moniker. Children are types, members, nested functions, etc. — whatever the language exposes.
- **Defs** : for each node in the tree, its `(moniker, kind, position)` triplet. `position` is `int4range` over byte offsets in source ; `NULL` when the module has no source text (synthetic / external).
- **Refs** : outgoing references. Each ref is `(source_moniker, target_moniker, kind, position)`. `source_moniker` is one of the module's own defs ; `target_moniker` may be any moniker in the canonical tree, in any module. `kind` distinguishes the relation (call, import, extends, uses_type, …). `position` is the location of the ref in source ; `NULL` when no source.

A `code_graph` is **immutable** as a value. Mutations are performed by constructors that return a new value.

Operators and functions :

| Function | Signature | Semantics |
|---|---|---|
| `graph_root` | `code_graph → moniker` | The module's own moniker. |
| `graph_contains` | `code_graph @> moniker → bool` | Does this graph define this moniker ? |
| `graph_defs` | `code_graph → setof (moniker, kind text, position int4range)` | Iterate defs. |
| `graph_refs` | `code_graph → setof (source moniker, target moniker, kind text, position int4range)` | Iterate refs. |
| `graph_locate` | `code_graph, moniker → int4range` | Position of a def in source. NULL if absent or no source. |
| `graph_def_monikers` | `code_graph → moniker[]` | Index inverse helper : flatten defs to a sortable array. |
| `graph_ref_targets` | `code_graph → moniker[]` | Index inverse helper : flatten outgoing ref targets. |

Constructors for synthetic graphs :

| Function | Signature | Semantics |
|---|---|---|
| `graph_create` | `(root moniker, kind text) → code_graph` | New graph rooted at this moniker. |
| `graph_add_def` | `(graph code_graph, m moniker, kind text, parent moniker, position int4range) → code_graph` | Add a def. parent must already be in the graph. |
| `graph_add_ref` | `(graph code_graph, source moniker, target moniker, kind text, position int4range) → code_graph` | Add a ref. source must be a def in the graph. |

Constructors return a new `code_graph` ; they do not mutate.

### Per-language extraction

One function per supported language, returning a `code_graph` from source :

```
extract_<lang>(uri text, source text, anchor moniker, presets jsonb) → code_graph
```

Arguments :
- `uri` — disk path or symbolic identifier of the source. Used by the extractor for diagnostics ; not embedded in produced monikers.
- `source` — the source text.
- `anchor` — the srcset moniker under which all extracted defs will be rooted. The extractor never produces monikers above this anchor.
- `presets` — language-specific configuration (e.g. JVM source root for Java, tsconfig paths for TS, namespace-package mode for Python). Caller-supplied, opaque to the extension's framework.

The extractor :
1. Parses `source` with the language's tree-sitter grammar.
2. Walks the AST.
3. Canonicalizes each def and ref into a moniker rooted at `anchor`.
4. Emits a `code_graph` value.

Supported languages are introduced one at a time. The MVP target is TypeScript. Java, Python, PL/pgSQL follow.

The extension is **stateless** : `extract_<lang>` reads only its arguments. It does not look up other modules, does not resolve refs across files. Cross-module resolution is the consumer's responsibility, performed by JOINing on the moniker type's `=` operator.

## Storage and linkage — canonical usage

The extension defines no tables. It provides the types (`moniker`, `code_graph`) and the operators ; the consumer chooses how to persist and index. The pattern below is the **canonical usage** the extension is designed to serve. ESAC implements a richer variant of it.

### Where a moniker belongs as a column

A `code_graph` already carries every moniker the module produces : its own root via `graph_root(graph)`, its defs via `graph_def_monikers(graph)`, its outgoing ref targets via `graph_ref_targets(graph)`. Reproducing any of these as a separate column on the holding row would duplicate the graph's content and open the door to incoherence (the column drifting from the graph it claims to summarise).

**A `moniker` column is justified only when the moniker is an edge endpoint that does not live inside any graph this row holds** — typically the `target_moniker` of a linkage edge, which by design points across modules. Everywhere else the moniker is derived from the graph at query time and indexed via expression indexes.

### One row per module

A module's structure is its `code_graph`. Source text is optional (NULL for `symbolic` / `external` origins). The minimal table :

```sql
CREATE TABLE module (
    id          uuid PRIMARY KEY,
    graph       code_graph NOT NULL,
    source_text text,
    source_uri  text,
    origin      text NOT NULL  -- 'extracted' | 'symbolic' | 'external'
);
```

The module's identity is `graph_root(graph)`. Make it queryable and unique with an expression index — not by introducing a redundant column :

```sql
CREATE UNIQUE INDEX module_root_uniq
    ON module ((graph_root(graph)));

CREATE INDEX module_root_gist
    ON module USING gist ((graph_root(graph)));
```

Update is row-replacement : changing a file produces a new `code_graph` and the row is rewritten atomically. No fragmented updates, no partial state.

### Linkage = JOIN on moniker

There is no separate linker phase. Resolving "which module defines this moniker" is a single indexed JOIN, expressed against the graph itself :

```sql
CREATE INDEX module_def_monikers_gin
    ON module USING gin (graph_def_monikers(graph));

SELECT m.* FROM module m
WHERE graph_def_monikers(m.graph) @> ARRAY['pcm://app/main#Foo#bar().'::moniker];
```

The reverse direction — every module that *references* a given target — is symmetric :

```sql
CREATE INDEX module_ref_targets_gin
    ON module USING gin (graph_ref_targets(graph));

SELECT m.* FROM module m
WHERE graph_ref_targets(m.graph) @> ARRAY['pcm://app/main#Foo#bar().'::moniker];
```

Both reduce to set-membership lookups, O(log n) on the corpus, with no row carrying any moniker as raw column data.

### Containment queries

Subtree queries on the canonical project tree leverage the moniker's `<@` / `@>` operators against the graph's root :

```sql
-- Every module under com.acme
SELECT m.* FROM module m
WHERE graph_root(m.graph) <@ 'pcm://app/main/com/acme'::moniker;

-- Every def under Foo
SELECT m.id, def.moniker FROM module m,
     LATERAL graph_defs(m.graph) AS def
WHERE def.moniker <@ 'pcm://app/main/com/acme/Foo'::moniker;
```

Backed by `module_root_gist` and the moniker GiST opclass (Phase 6).

### Linkage table — where moniker columns belong

For hot queries (`find-callers` repeated millions of times, deep call-graph traversal) the linkage can be projected once into a flat cache. **This is the table where storing monikers as columns is the right shape** : each row is a single edge whose endpoints are individual monikers, not summaries of a graph.

```sql
CREATE TABLE linkage (
    source_id      uuid       NOT NULL REFERENCES module(id) ON DELETE CASCADE,
    source_moniker moniker    NOT NULL,
    target_moniker moniker    NOT NULL,
    kind           text       NOT NULL,
    position       int4range
);

CREATE INDEX linkage_target_gist ON linkage USING gist (target_moniker);
CREATE INDEX linkage_source     ON linkage (source_id);
```

Populated from the `module` rows :

```sql
INSERT INTO linkage (source_id, source_moniker, target_moniker, kind, position)
SELECT m.id, r.source, r.target, r.kind, r.position
FROM module m, LATERAL graph_refs(m.graph) AS r;
```

This table is **reconstructible from the `code_graph` rows at any time** ; it is a cache, not the truth. Truth lives in `module.graph`. Triggers can keep `linkage` in sync (delete-then-insert on module update) but they are a consumer concern, not part of the extension.

### Origins

The same `module` table holds all three origins uniformly. Discriminate by `origin` when needed :

- `extracted` — `source_text` and `source_uri` non-NULL ; positions in `graph` are real.
- `symbolic` — both NULL ; positions in `graph` are NULL.
- `external` — both NULL ; `graph` may be minimal.

Promotion (e.g. `symbolic` → `extracted` when a source file appears) is an UPDATE on the row that swaps `graph` and fills `source_*`. The module's `id` is preserved ; the root moniker is the same one it held before, since it is computed from the new graph that was built to match. Inbound refs survive automatically.

## API surface (public)

The SQL surface is generated by pgrx from `#[pg_extern]` and `#[derive(PostgresType)]` annotations on Rust items. Conceptually:

- **Types** : `moniker`, `moniker_pattern`, `code_graph`.
- **Operators** :
  - `moniker = moniker → bool`
  - `moniker <@ moniker → bool`, `moniker @> moniker → bool`
  - `moniker || (segment, kind) → moniker`
  - `moniker ~ moniker_pattern → bool`
  - `code_graph @> moniker → bool`
- **Accessors** on `moniker` : `kind_of`, `project_of`, `path_of`, `parent_of`, `depth`.
- **Accessors / iterators** on `code_graph` : `graph_root`, `graph_defs`, `graph_refs`, `graph_locate`, `graph_def_monikers`, `graph_ref_targets`.
- **Constructors** : `graph_create(moniker, kind) → code_graph`, `graph_add_def(...)`, `graph_add_ref(...)`. Immutable: each returns a new `code_graph`.
- **Per-language extractors** : `extract_typescript(uri, source, anchor, presets) → code_graph`. One per supported language ; added incrementally.
- **Index** : custom GiST opclass for `moniker` (`=`, `<@`, `@>`, `~`).

All functions are `IMMUTABLE STRICT PARALLEL SAFE` unless explicitly noted.

## Implementation order

Six phases. Each phase produces a runnable, testable artifact ; subsequent phases extend, do not refactor. Implementation follows TDD : tests describe behaviour before the code that satisfies them.

### Phase 1 — `moniker` type minimal

- The `moniker` type with URI in/out, equality, and the four-class punctuation discipline (Path/Type/Term/Method).
- Accessors : `kind_of`, `project_of`, `path_of`, `parent_of`, `depth`.
- Tests : URI roundtrip on representative cases (Java FQN, TS module path, external pkg).

Out of scope : GiST, containment operators, pattern matching, composition.

Deliverable : `SELECT 'pcm://my-app/main/com/acme/Foo'::moniker = ...` works.

### Phase 2 — `code_graph` type

- The `code_graph` type with `graph_create`, `graph_add_def`, `graph_add_ref` constructors.
- Accessors : `graph_root`, `graph_defs`, `graph_refs`, `graph_locate`.
- Containment operator : `code_graph @> moniker`.
- Helpers : `graph_def_monikers`, `graph_ref_targets`.

Deliverable : a `code_graph` can be constructed in pure SQL via constructors and queried.

### Phase 3 — per-language extractor (TypeScript first)

- Tree-sitter integration ; per-language grammar crate.
- `extract_typescript(uri, source, anchor, presets)` walks the AST and produces a `code_graph`.
- Canonicalisation of TS monikers ; refs extraction (imports, calls, type uses, extends/implements).
- Tests : fixture TS files → expected `code_graph` snapshots.

Out of scope : other languages, advanced TS features (decorators, generics inference) — handled progressively.

Deliverable : `extract_typescript(...)` returns a populated `code_graph`.

### Phase 4 — query the graph via moniker

- `code_graph @> moniker` performance hardening.
- Compose with `moniker` operators : `graph @> (parent_moniker || ('foo', 'method'))` etc.
- Pattern matching against the graph's defs.
- Tests : containment queries on the TS-extracted graphs from phase 3.

Deliverable : graph membership and lookup are fast.

### Phase 5 — prototype linkage between two graphs

- Two TS source files, one with a ref to the other.
- Demonstrate the link via SQL operators alone (no cross-call resolution in the extractor).
- Tests : two-graph scenarios for the canonical kinds (call, import, extends).

Deliverable : two-graph linkage works end-to-end via the extension's operators alone.

### Phase 6 — GiST index

- Custom GiST opclass for `moniker` (`=`, `<@`, `@>`, `~`).
- Benchmarks : equality lookup, containment scan, ref resolution on N-graph corpora.

Deliverable : indexed queries are O(log n) on representative corpora.

## Testing

Two layers, both run from the same repo:

- **Pure-Rust unit tests** (`cargo test`) — exercise `core/` and `lang/` directly with no PG running. Fast (sub-second), debuggable with regular tools, cannot crash a backend. Cover URI roundtrip, kind interning, builders, operators, extractor canonicalization.
- **In-PG integration tests** (`cargo pgrx test pgN`, behind the `pg_test` feature) — boot a per-version PG, install the extension, exercise the SQL surface end-to-end. Cover type registration, operator dispatch, index correctness, GiST opclass behaviour. Used sparingly, only for what the pure-Rust layer cannot validate.

Fixture-based extractor tests live under `tests/fixtures/<lang>/` ; expected `code_graph` outputs are snapshotted as Rust constants. Performance benchmarks use synthetic graphs at 10⁴, 10⁵, 10⁶ defs/refs.

## Compatibility

- PostgreSQL 14, 15, 16, 17. Each supported via a Cargo feature (`pg14` … `pg17`); only one is active at build time.
- Rust 1.90+ (edition 2024).
- pgrx 0.18+.
- Tree-sitter via the `tree-sitter` crate and per-language grammar crates fetched by Cargo.

## Open decisions

These are deferred to implementation, not invariants of the spec.

- Whether `moniker_pattern` is a compiled form or a text-glob.
- Variadic / array-form constructors (`graph_add_defs(code_graph, def_record[])`) for bulk loading.
- Persistence and portability of interned kind metadata across PG backends.

These can be revisited without breaking the public API.

## Non-scope

- **Stack-graphs / dynamic resolution.** The extension targets languages where the moniker is locally determinable. If a language with non-determinable scoping must be supported later, stack-graphs is the architectural fallback — a separate design.
- **Cross-project federation.** Today a moniker is scoped to a project via its authority. Federation between projects (a same external shared between consumers, cross-project refs) is an open question for another framework.
- **Concurrent collaborative editing of a synthetic `code_graph`.** The model assumes a single writer per value.
