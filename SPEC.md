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

The srcset is the boundary node. Its kind is supplied by the caller (`srcset`, `workspace_app`, etc.). Below it, the **first segment of the language regime is `lang:<short>`**, posted by the extractor (`lang:ts`, `lang:rs`, `lang:java`, `lang:python`, `lang:sql`). The extension does not interpret srcsets — it receives the srcset moniker as an anchor and emits all source-derived nodes under it, prefixed by the language segment.

External nodes (`external_pkg:...`) live entirely in the project regime and have no language segment.

### The moniker

A node's identity in the canonical tree. Native PostgreSQL type with operators and a custom GiST index.

Canonical external representation is a **typed-segment URI** using a `+moniker` scheme profile:

```
<scheme>+moniker://<project>/<kind>:<name>[/<kind>:<name>...][#<kind>:<name>[#<kind>:<name>...]]
```

The base scheme is caller-configurable (default `pcm`, canonical URI `pcm+moniker://`); a consumer like ESAC uses its own (`esac`, canonical URI `esac+moniker://`). The `+moniker` suffix identifies the canonical typed moniker profile, not the final symbol kind. Kinds are carried by each segment.

Examples (default scheme):

```
pcm+moniker://my-app/srcset:main/lang:java/package:com/package:acme/class:Foo
pcm+moniker://my-app/srcset:main/lang:java/package:com/package:acme/class:Foo#method:bar()
pcm+moniker://my-app/srcset:main/lang:java/package:com/package:acme/class:Foo#method:bar(int,String)
pcm+moniker://my-app/srcset:main/lang:ts/dir:src/dir:lib/module:util#class:Helper#method:process()
pcm+moniker://my-app/srcset:main/lang:python/package:acme/module:util#class:Helper#method:process(int)
pcm+moniker://my-app/srcset:db/lang:sql/schema:esac/module:plan#function:create_plan(uuid,text)
pcm+moniker://my-app/external_pkg:maven/org.springframework/spring-core/6.1.0
```

The earlier SCIP-like punctuation form remains a compact display option under the base scheme (`pcm://`, `esac://`); it is lossy and not a persistence format.

The canonical typed `+moniker` form is the durable I/O contract.

The URI is symbolic only — it identifies the node in the tree. It does not encode disk location. Source location remains a sidecar (`source_uri`) on the module row.

Names with reserved characters (`/`, `#`, `:`, `(`, `)`, backtick, whitespace) are wrapped in backticks; literal backticks are doubled.

Detailed URI design and segment semantics are documented in `docs/MONIKER_URI.md`.

#### Operators

| Op            | Signature                                      | Semantics                                                                |
|---------------|------------------------------------------------|--------------------------------------------------------------------------|
| `=`           | `moniker = moniker → bool`                     | Byte-strict equality (every segment, including final kind). Total identity. |
| `bind_match`  | `bind_match(moniker, moniker) → bool`          | Structural matching for cross-file linkage. **The linkage primitive.**   |
| `<@`          | `moniker <@ moniker → bool`                    | Left is descendant of right.                                             |
| `@>`          | `moniker @> moniker → bool`                    | Left is ancestor of right.                                               |
| `\|\|`        | `moniker \|\| (segment text, kind text) → moniker` | Compose child from parent.                                            |
| `~`           | `moniker ~ moniker_pattern → bool`             | Pattern match.                                                           |

`bind_match(left, right)` is true when:

- `left.project` byte-equal to `right.project`;
- every segment of `left` and `right` except the last is byte-equal (which includes `srcset:` and `lang:` — cross-language matches are by design impossible);
- the **last** segment compares **name-only**: `left.last.name == right.last.name`. The last segment's `kind` may differ.

`bind_match` solves the problem that an extractor is local and does not know the kind of a symbol it imports from another file. Imports emit a placeholder last-segment kind (typically `path` or a best-effort kind); the exporting module's def carries the true kind. Byte-strict `=` would never match those two; `bind_match` does.

Default last-segment matching is byte-strict on the name. Language-specific refinements (e.g. callable bare-name matching for cases like `function:Y()` × `function:Y(int,str)`) are layered as needed and routed by reading the shared `lang:` segment; the routing surface is reserved, the default strategy is strict.

Accessors : `kind_of(moniker) → text`, `project_of(moniker) → text`, `lang_of(moniker) → text` (returns the `lang:` segment payload, or empty for project-regime monikers), `path_of(moniker) → text[]`, `parent_of(moniker) → moniker`, `depth(moniker) → int`.

I/O functions : `moniker_in(cstring) → moniker`, `moniker_out(moniker) → cstring`. URI parsing and serialization are part of the type's I/O contract.

GiST opclass : custom access method indexing `=`, `bind_match`, `<@`, `@>`, `~`.

ODR (One Definition Rule) is a property the consumer enforces with `UNIQUE` constraints on columns of type `moniker`. The extension does not police ODR — it provides the type whose equality makes ODR enforceable.

### The code_graph

A native type carrying the internal structure of a single module.

A `code_graph` contains :

- **Tree** : the intra-module containment hierarchy. Root is the module's own moniker. Children are types, members, nested functions, etc. — whatever the language exposes.
- **Defs** : for each node in the tree, its record carrying `(moniker, kind, parent, position, visibility, signature, binding, origin)`. `position` is `int4range` over byte offsets in source ; `NULL` when the module has no source text (synthetic / external). `origin` distinguishes how the def was produced (see § Origin semantics).
- **Refs** : outgoing references. Each ref carries `(source_moniker, target_moniker, kind, position, receiver_hint, alias, confidence, binding)`. `source_moniker` is one of the module's own defs ; `target_moniker` may be any moniker in the canonical tree, in any module. `kind` distinguishes the relation (call, import, extends, uses_type, …). `position` is the location of the ref in source ; `NULL` when no source.

A `code_graph` is **immutable** as a value. Mutations are performed by constructors that return a new value.

#### Binding semantics

Binding is the row-level qualifier for cross-file linkage. It is **not** in the moniker bytes — it is a column on `DefRecord` and `RefRecord`.

**`DefRecord.binding`** ∈ {`export`, `local`, `none`, `inject`} :

| Value     | When                                                                                                |
|-----------|-----------------------------------------------------------------------------------------------------|
| `export`  | Symbol is addressable cross-module. `visibility` ∈ {`public`, `protected`, `package`} and `kind` ∉ {`local`, `param`, `section`}. Modules themselves are `export`. |
| `local`   | Symbol is module-scoped. `visibility` ∈ {`private`, `module`} or `kind` ∈ {`local`, `param`}.       |
| `inject`  | Symbol is a DI provider/target (e.g. `@Injectable`, `@Service`, `@Bean`, NestJS providers). Resolved by a container at runtime, not by static `import`. |
| `none`    | Concept does not apply (`kind=section`).                                                            |

**`RefRecord.binding`** ∈ {`import`, `local`, `none`, `inject`} :

| Value     | When                                                                                                |
|-----------|-----------------------------------------------------------------------------------------------------|
| `import`  | Ref points to another module via static import. `kind` ∈ {`imports_symbol`, `imports_module`, `reexports`}. The primary input to `bind_match`. |
| `local`   | Ref points inside the current module. Resolves via byte-strict `=` against the same module's defs.  |
| `inject`  | Ref demands a binding via DI container. `kind` ∈ {`di_register`, `di_require`}, plus constructor params whose annotated type is a known DI service. |
| `none`    | Ref's nature does not categorize as linkage (unresolved calls, reads of unknown identifiers).       |

For `bind_match` purposes there is **no semantic distinction** between `import` and `inject`. The matching table is binary:

```
ref.binding ∈ {import, inject}  ×  def.binding ∈ {export, inject}
```

`inject` is a qualification for downstream traceability (which links go through a DI container vs static import), not a matching axis. Consumers that want to filter DI separately project on `binding`.

#### Origin semantics

Origin is the row-level qualifier for **how a def came into existence**. It is a column on `DefRecord`, opaque to `bind_match`.

`DefRecord.origin` ∈ {`extracted`, `declared`, `inferred`} :

| Value       | When                                                                                                                                              |
|-------------|---------------------------------------------------------------------------------------------------------------------------------------------------|
| `extracted` | Produced by an `extract_<lang>` call from real source. Default for all extractor output. Positions are real.                                       |
| `declared`  | Produced by `code_graph_declare` from a declarative spec (see § Declarative graphs). The symbol exists at the moniker level only — no implementation. Positions are NULL. |
| `inferred`  | Produced by analytical projections (e.g. types implied by usage with no source-level def). Reserved for future use.                                |

Origin does **not** participate in `bind_match`. A `declared` def and an `extracted` def with the same moniker resolve identically. When both exist for one moniker (typical when a spec is later implemented), the consumer applies its own precedence — usually `extracted > declared`.

`RefRecord` has no `origin` column. A ref's provenance follows its containing graph's defs.

#### Operators and functions

| Function | Signature | Semantics |
|---|---|---|
| `graph_root` | `code_graph → moniker` | The module's own moniker. |
| `graph_contains` | `code_graph @> moniker → bool` | Does this graph define this moniker ? |
| `graph_defs` | `code_graph → setof DefRecord` | Iterate defs (moniker, kind, parent, position, visibility, signature, binding). |
| `graph_refs` | `code_graph → setof RefRecord` | Iterate refs (source, target, kind, position, receiver_hint, alias, confidence, binding). |
| `graph_locate` | `code_graph, moniker → int4range` | Position of a def in source. NULL if absent or no source. |
| `graph_def_monikers` | `code_graph → moniker[]` | Index inverse helper : flatten defs to a sortable array. |
| `graph_ref_targets` | `code_graph → moniker[]` | Index inverse helper : flatten outgoing ref targets. |
| `graph_export_monikers` | `code_graph → moniker[]` | Defs whose `binding` ∈ {`export`, `inject`}. Linkage-side index helper. |
| `graph_import_targets` | `code_graph → moniker[]` | Refs whose `binding` ∈ {`import`, `inject`}. Linkage-side index helper. |

Constructors for synthetic graphs :

| Function | Signature | Semantics |
|---|---|---|
| `graph_create` | `(root moniker, kind text) → code_graph` | New graph rooted at this moniker. |
| `graph_add_def` | `(graph code_graph, m moniker, kind text, parent moniker, position int4range, visibility text, signature text, binding text, origin text) → code_graph` | Add a def. parent must already be in the graph. `origin` defaults to `extracted` when empty. |
| `graph_add_ref` | `(graph code_graph, source moniker, target moniker, kind text, position int4range, receiver_hint text, alias text, confidence text, binding text) → code_graph` | Add a ref. source must be a def in the graph. |
| `code_graph_declare` | `(spec jsonb) → code_graph` | Build a graph from a declarative specification (see § Declarative graphs). All defs marked `origin=declared`. |
| `code_graph_to_spec` | `(graph code_graph) → jsonb` | Reverse projection: emit the JSONB spec of a graph (lossy on non-canonical ref kinds). The `lang` field is inferred from the root's `lang:` segment. |

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
- `presets` — language-specific configuration. Caller-supplied, opaque to the extension's framework.

The extractor :
1. Parses `source` with the language's tree-sitter grammar.
2. Walks the AST.
3. **Posts the `lang:<short>` segment** as the first segment under `anchor`. Every produced moniker is rooted under `anchor/lang:<short>/...`.
4. Canonicalises each def and ref into a moniker rooted at `anchor`.
5. Tags `binding` on every def (`export` / `local` / `inject` / `none`) and every ref (`import` / `local` / `inject` / `none`) per the rules in § Binding semantics.
6. Emits a `code_graph` value.

Supported languages: TypeScript, Rust, Java, Python, SQL/PL-pgSQL.

The extension is **stateless** : `extract_<lang>` reads only its arguments. It does not look up other modules, does not resolve refs across files. Cross-module resolution is the consumer's responsibility, performed by JOINing on `bind_match` (cross-file) or `=` (intra-file or total identity).

### Declarative graphs

A `code_graph` may be authored without source. The declarative constructor `code_graph_declare(spec jsonb) → code_graph` accepts a JSONB specification of symbols and edges and emits a `code_graph` indistinguishable in shape from extractor output. Every produced def carries `origin=declared`.

Use cases :
- **Forward modeling** — declare a symbol before implementing it ; consumers see it appear in cross-file linkage immediately.
- **External libraries with no source** — declare the public surface so calls into them resolve via `bind_match`.
- **Architecture validation** — declare the intended graph and diff against actual extraction.

#### Canonical edge alphabet

Declarative edges use a canonical vocabulary aligned with the three relations carried at the moniker level :

| Edge kind          | Maps to `REF_*` kind | Default binding                                              | Semantics                                  |
|--------------------|----------------------|--------------------------------------------------------------|--------------------------------------------|
| `depends_on`       | `imports_module`     | `import`                                                     | Source needs target at load time.          |
| `calls`            | `calls`              | `local` if `from` and `to` share the `module:` segment, else `none` | Source invokes target at runtime.   |
| `injects:provide`  | `di_register`        | `inject`                                                     | Source registers target in a DI container. |
| `injects:require`  | `di_require`         | `inject`                                                     | Source consumes target via DI container.   |

The richer extraction-time vocabulary (`uses_type`, `extends`, `implements`, `reads`, `annotates`, `imports_symbol`, `reexports`, `instantiates`, `method_call`) is **not** accepted by the declarative constructor. Specs operate at the abstraction level of the canonical model.

#### Spec format

A spec is a JSONB document validated against `docs/declare_schema.json` (JSON Schema 2020-12). Top-level shape:

```json
{
  "root":    "<MonikerURI of the root module>",
  "lang":    "ts | rs | java | python | go | cs | sql",
  "symbols": [
    { "moniker":    "<MonikerURI>",
      "kind":       "<lang-specific kind, see profile table>",
      "parent":     "<MonikerURI of an existing symbol or the root>",
      "visibility": "<lang-specific visibility, see profile table>",
      "signature":  "<type-only normalized signature, optional>" }
  ],
  "edges":   [
    { "from": "<MonikerURI of a declared symbol>",
      "kind": "depends_on | calls | injects:provide | injects:require",
      "to":   "<MonikerURI, may reference symbols outside this spec>" }
  ]
}
```

The shape is uniform across languages. Per-language profiles in the schema restrict only `kind` and `visibility` enumerations :

| lang     | accepted `kind` values                                                                              | accepted `visibility` values            |
|----------|-----------------------------------------------------------------------------------------------------|-----------------------------------------|
| `ts`     | class, interface, type, function, method, const, namespace, module, enum                            | public, private, module                 |
| `rs`     | struct, enum, trait, impl, fn, method, const, static, mod, type                                     | public, private, module                 |
| `java`   | class, interface, enum, record, annotation_type, method, constructor, field                         | public, protected, package, private     |
| `python` | class, function, method, async_function                                                             | public, private, module                 |
| `go`     | type, struct, interface, func, method, var, const                                                   | public, module                          |
| `cs`     | class, interface, struct, record, enum, delegate, method, constructor, field, property, event       | public, protected, internal, private    |
| `sql`    | function, procedure, view, table, schema                                                            | (visibility ignored)                    |

The visibility vocabulary is **cross-language** : `module` covers package-private (Go), module-internal (Rust), nested-but-not-re-exported (TS, Python). It maps to `VIS_MODULE` in the extractor output, so the round-trip `extract → code_graph_to_spec → code_graph_declare` stays valid for every supported language.

#### Validation

At ingest, `code_graph_declare` :

1. Validates `spec` against `docs/declare_schema.json` (structural shape + per-lang enum restrictions).
2. Parses each `MonikerURI` as a typed moniker.
3. Checks that `symbols[i].kind` matches the last segment kind of `symbols[i].moniker` (semantic agreement between metadata and moniker bytes).
4. Checks that `symbols[i].parent` is `root` or another declared symbol.
5. Rejects duplicate monikers in `symbols`.
6. Checks that `edges[i].from` references a declared symbol.
7. Builds the graph and returns it.

`edges[i].to` is **not** required to exist in the spec. The whole point of declarative graphs is to reference symbols that may not exist yet ; `bind_match` resolves at query time against whatever defs are in the corpus.

#### Reverse projection

`code_graph_to_spec(graph) → jsonb` is the inverse of `code_graph_declare`. It walks a `code_graph` and emits a JSONB document in the same shape as the input spec :

- The `lang` field is inferred from the root's `lang:` segment ; if absent, the function errors.
- Every non-root def becomes a `symbols[i]` entry. `visibility` and `signature` are emitted only when non-empty. Positions and `origin` are dropped (they are not part of the spec format).
- Refs are filtered to the canonical edge alphabet : `imports_module → depends_on`, `calls → calls`, `di_register → injects:provide`, `di_require → injects:require`. **All other ref kinds are silently dropped** (`uses_type`, `extends`, `implements`, `reads`, `annotates`, `imports_symbol`, `reexports`, `instantiates`, `method_call`).

**Round-trip guarantee** : for any graph `g` produced by `code_graph_declare(s)`, the round-trip `code_graph_declare(code_graph_to_spec(g))` produces a graph equivalent to `g`. The function is lossy on extracted graphs whose refs use non-canonical kinds — by design ; specs operate at a higher abstraction level than extractor output.

#### Examples

Java — declare a service class with a call to an external dep :

```json
{
  "root": "pcm+moniker://my-app/srcset:main/lang:java/package:com/package:acme/module:UserService",
  "lang": "java",
  "symbols": [
    { "moniker":    "pcm+moniker://my-app/srcset:main/lang:java/package:com/package:acme/module:UserService#class:UserService",
      "kind":       "class",
      "parent":     "pcm+moniker://my-app/srcset:main/lang:java/package:com/package:acme/module:UserService",
      "visibility": "public" },
    { "moniker":    "pcm+moniker://my-app/srcset:main/lang:java/package:com/package:acme/module:UserService#class:UserService#method:findByEmail(String)Optional",
      "kind":       "method",
      "parent":     "pcm+moniker://my-app/srcset:main/lang:java/package:com/package:acme/module:UserService#class:UserService",
      "visibility": "public",
      "signature":  "findByEmail(String): Optional" }
  ],
  "edges": [
    { "from": "pcm+moniker://my-app/srcset:main/lang:java/package:com/package:acme/module:UserService#class:UserService#method:findByEmail(String)Optional",
      "kind": "calls",
      "to":   "pcm+moniker://my-app/external_pkg:maven/jakarta.persistence/jakarta.persistence-api/EntityManager#method:find(Class,Object)Object" }
  ]
}
```

TypeScript — same shape, restricted enums :

```json
{
  "root": "pcm+moniker://my-app/srcset:main/lang:ts/dir:src/dir:services/module:user-service",
  "lang": "ts",
  "symbols": [
    { "moniker":    "pcm+moniker://my-app/srcset:main/lang:ts/dir:src/dir:services/module:user-service#class:UserService",
      "kind":       "class",
      "parent":     "pcm+moniker://my-app/srcset:main/lang:ts/dir:src/dir:services/module:user-service",
      "visibility": "public" },
    { "moniker":    "pcm+moniker://my-app/srcset:main/lang:ts/dir:src/dir:services/module:user-service#class:UserService#method:findByEmail(string)Promise",
      "kind":       "method",
      "parent":     "pcm+moniker://my-app/srcset:main/lang:ts/dir:src/dir:services/module:user-service#class:UserService",
      "visibility": "public",
      "signature":  "findByEmail(string): Promise" }
  ],
  "edges": [
    { "from": "pcm+moniker://my-app/srcset:main/lang:ts/dir:src/dir:services/module:user-service#class:UserService",
      "kind": "depends_on",
      "to":   "pcm+moniker://my-app/external_pkg:npm/typeorm/Repository" }
  ]
}
```

Rust — `pub fn` requiring a trait via DI wiring :

```json
{
  "root": "pcm+moniker://my-app/srcset:main/lang:rs/mod:domain/mod:user/module:service",
  "lang": "rs",
  "symbols": [
    { "moniker":    "pcm+moniker://my-app/srcset:main/lang:rs/mod:domain/mod:user/module:service#fn:create_user(String,String)Result",
      "kind":       "fn",
      "parent":     "pcm+moniker://my-app/srcset:main/lang:rs/mod:domain/mod:user/module:service",
      "visibility": "public",
      "signature":  "create_user(String, String): Result" }
  ],
  "edges": [
    { "from": "pcm+moniker://my-app/srcset:main/lang:rs/mod:domain/mod:user/module:service#fn:create_user(String,String)Result",
      "kind": "injects:require",
      "to":   "pcm+moniker://my-app/srcset:main/lang:rs/mod:infra/module:db#trait:UserRepo" }
  ]
}
```

## Storage and linkage — canonical usage

The extension defines no tables. It provides the types (`moniker`, `code_graph`) and the operators ; the consumer chooses how to persist and index. The pattern below is the **canonical usage** the extension is designed to serve. ESAC implements a richer variant of it.

### Where a moniker belongs as a column

A `code_graph` already carries every moniker the module produces : its own root via `graph_root(graph)`, its defs via `graph_def_monikers(graph)`, its outgoing ref targets via `graph_ref_targets(graph)`. Reproducing any of these as a separate column on the holding row would duplicate the graph's content and open the door to incoherence (the column drifting from the graph it claims to summarise).

**A `moniker` column is justified only when the moniker is an edge endpoint that does not live inside any graph this row holds** — typically the `target_moniker` of a linkage edge, which by design points across modules. Everywhere else the moniker is derived from the graph at query time and indexed via expression indexes.

### One row per module

A module's structure is its `code_graph`. Source text is optional (NULL for `declared` / `external` origins). The minimal table :

```sql
CREATE TABLE module (
    id          uuid PRIMARY KEY,
    graph       code_graph NOT NULL,
    source_text text,
    source_uri  text,
    origin      text NOT NULL  -- 'extracted' | 'declared' | 'external'
);
```

The module's identity is `graph_root(graph)`. Make it queryable and unique with an expression index :

```sql
CREATE UNIQUE INDEX module_root_uniq
    ON module ((graph_root(graph)));

CREATE INDEX module_root_gist
    ON module USING gist ((graph_root(graph)));
```

Update is row-replacement : changing a file produces a new `code_graph` and the row is rewritten atomically.

### Linkage = JOIN on bind_match

Cross-file linkage is a single indexed JOIN expressed against the graph itself. Use `bind_match` for the cross-file case (the extractor doesn't know the target's exact kind) and `=` for total identity.

```sql
CREATE INDEX module_export_gin
    ON module USING gin (graph_export_monikers(graph));

CREATE INDEX module_import_gin
    ON module USING gin (graph_import_targets(graph));

-- Find every module that exports a binding the current import points to.
SELECT m_def.id, m_imp.id
FROM module m_imp,
     LATERAL graph_refs(m_imp.graph) r,
     module m_def,
     LATERAL graph_defs(m_def.graph) d
WHERE r.binding IN ('import', 'inject')
  AND d.binding IN ('export', 'inject')
  AND bind_match(r.target, d.moniker);
```

The index supports `bind_match` via the moniker GiST opclass; lookups remain O(log n) on the corpus.

### Containment queries

Subtree queries on the canonical project tree leverage the moniker's `<@` / `@>` operators against the graph's root :

```sql
-- Every module under com.acme (any language)
SELECT m.* FROM module m
WHERE graph_root(m.graph) <@ 'pcm+moniker://app/srcset:main'::moniker;

-- Every module of the Java regime under a srcset
SELECT m.* FROM module m
WHERE graph_root(m.graph) <@ 'pcm+moniker://app/srcset:main/lang:java'::moniker;
```

Backed by `module_root_gist` and the moniker GiST opclass.

### Linkage table — where moniker columns belong

For hot queries (`find-callers` repeated millions of times, deep call-graph traversal) the linkage can be projected once into a flat cache. **This is the table where storing monikers as columns is the right shape** : each row is a single edge whose endpoints are individual monikers, not summaries of a graph.

```sql
CREATE TABLE linkage (
    source_id      uuid       NOT NULL REFERENCES module(id) ON DELETE CASCADE,
    source_moniker moniker    NOT NULL,
    target_moniker moniker    NOT NULL,
    kind           text       NOT NULL,
    binding        text       NOT NULL,
    confidence     text       NOT NULL,
    position       int4range
);

CREATE INDEX linkage_target_gist ON linkage USING gist (target_moniker);
CREATE INDEX linkage_source     ON linkage (source_id);
```

Populated from the `module` rows :

```sql
INSERT INTO linkage (source_id, source_moniker, target_moniker, kind, binding, confidence, position)
SELECT m.id, r.source, r.target, r.kind, r.binding, r.confidence, r.position
FROM module m, LATERAL graph_refs(m.graph) AS r;
```

This table is **reconstructible from the `code_graph` rows at any time** ; it is a cache, not the truth. Truth lives in `module.graph`.

### Origins

The same `module` table holds all three origins uniformly. Discriminate by `origin` when needed :

- `extracted` — `source_text` and `source_uri` non-NULL ; positions in `graph` are real ; `graph_defs(graph)` rows have `origin=extracted`.
- `declared` — both NULL ; positions in `graph` are NULL ; produced by `code_graph_declare` ; `graph_defs(graph)` rows have `origin=declared`.
- `external` — both NULL ; `graph` may be minimal.

Module-level `origin` and def-level `origin` (on `DefRecord`) share the same vocabulary by design.

Promotion (e.g. `declared` → `extracted` when a source file appears) is an UPDATE on the row that swaps `graph` and fills `source_*`. The module's `id` is preserved.

## API surface (public)

The SQL surface is generated by pgrx from `#[pg_extern]` and `#[derive(PostgresType)]` annotations on Rust items. Conceptually:

- **Types** : `moniker`, `moniker_pattern`, `code_graph`.
- **Operators** :
  - `moniker = moniker → bool`
  - `bind_match(moniker, moniker) → bool`
  - `moniker <@ moniker → bool`, `moniker @> moniker → bool`
  - `moniker || (segment, kind) → moniker`
  - `moniker ~ moniker_pattern → bool`
  - `code_graph @> moniker → bool`
- **Accessors** on `moniker` : `kind_of`, `project_of`, `lang_of`, `path_of`, `parent_of`, `depth`.
- **Accessors / iterators** on `code_graph` : `graph_root`, `graph_defs`, `graph_refs`, `graph_locate`, `graph_def_monikers`, `graph_ref_targets`, `graph_export_monikers`, `graph_import_targets`.
- **Constructors** : `graph_create(moniker, kind) → code_graph`, `graph_add_def(...)`, `graph_add_ref(...)`, `code_graph_declare(jsonb) → code_graph`. Immutable: each returns a new `code_graph`.
- **Projection** : `code_graph_to_spec(code_graph) → jsonb`. Inverse of `code_graph_declare`, lossy on non-canonical ref kinds.
- **Per-language extractors** : `extract_typescript(...)`, `extract_rust(...)`, `extract_java(...)`, `extract_python(...)`, `extract_plpgsql(...)`. One per supported language.
- **Index** : custom GiST opclass for `moniker` (`=`, `bind_match`, `<@`, `@>`, `~`).

All functions are `IMMUTABLE STRICT PARALLEL SAFE` unless explicitly noted.

## Implementation phases

Phases 1–6 shipped (typed URI, code_graph, five extractors, GiST opclass, compact projection, dogfood panel). The current effort is **Phase 7 — `bind_match` + binding metadata + `lang:` segment**.

### Phase 7 — Cross-file linkage

Three coordinated changes that unlock cross-file linkage:

1. **`lang:` segment** posted by every extractor as the first segment under the anchor (`compute_module_moniker` in each `src/lang/<lang>/canonicalize.rs`).
2. **`binding` column** on `DefRecord` and `RefRecord`, with extractor logic to tag every produced row per § Binding semantics.
3. **`bind_match` operator** registered on the moniker GiST opclass, with a recheck function and a strategy number distinct from `=`.

#### Per-language import target shape

For `bind_match` to JOIN an import ref against the corresponding export def, the import target's all-but-last segments must be byte-equal to the def's all-but-last segments. Each extractor lowers its language's import syntax to a target moniker that respects this:

- **TS / JS** — relative imports (`./foo`) inherit the importer's view (already in `lang:ts/`) and walk dirs as `path:`. Bare specifiers (`react`) land in the project regime under `external_pkg:`. No extra wiring needed.
- **Python** — absolute project-local imports (`from acme.util import X`) build under `lang:python/package:.../module:.../path:X`. Relative imports (`from ._models import Response`) walk up the importer's module chain by `leading_dots-1` package levels and then attach the requested pieces. Stdlib (`json`, `os`, …) keeps the project-regime `external_pkg:` shape.
- **Rust** — `crate::` builds under `lang:rs/`. The second-to-last piece becomes `module:<name>`, the last piece is `path:<symbol>`. `super::` / `self::` use the importer's view. Re-export chains (`use crate::a::b::c` where `c` is a `pub use` from a deeper module) cannot be resolved locally — the consumer's projection layer follows the chain.
- **Java** — named imports (`import com.acme.Foo`) build under `lang:java/package:com/package:acme/module:Foo/path:Foo`. The last piece is duplicated as both `module:` (the file) and `path:` (the symbol) so `bind_match` unifies with `module:Foo/class:Foo`. JDK packages (`java.*`, `javax.*`) keep the `external_pkg:` shape.
- **SQL / PL-pgSQL** — all `calls` refs are tagged `binding=local` (intra-module by language design) and use arity-only callable names like `function:bar(2)`, while defs use typed names like `function:bar(int4,text)`. `bind_match`'s byte-strict last-segment-name rule does not unify these. Cross-file SQL call linkage requires a per-language refinement of `bind_match` (callable bare-name comparison routed via the `lang:sql` segment) — deferred to a follow-up effort.

Validation: cross-file linkage on the dogfood panel resolves at scale across TS, Java, Python, and Rust:

| project          | lang  | files | resolved links |
|------------------|-------|-------|----------------|
| date-fns         | ts    | 1342  | 2115           |
| gson             | java  | 83    | 286            |
| httpx            | py    | 24    | 131            |
| zod              | ts    | 82    | 110            |
| pg_code_moniker  | rs    | 58    | 19             |
| pgtap            | sql   | 12    | 0 (deferred)   |

## Testing

Two layers, both run from the same repo:

- **Pure-Rust unit tests** (`cargo test`) — exercise `core/` and `lang/` directly with no PG running. Fast (sub-second), debuggable with regular tools, cannot crash a backend. Cover URI roundtrip, builders, operators, extractor canonicalization, binding tag rules.
- **pgTAP integration tests** (`./test/run.sh`) — boot the pgrx-managed PG17, install the extension, exercise the SQL surface end-to-end. Cover type registration, operator dispatch, index correctness, `bind_match` behaviour against the GiST opclass.

Fixture-based extractor tests live in `#[cfg(test)] mod tests` next to each extractor. Performance benchmarks use real source files via `examples/bench_*`.

## Compatibility

- PostgreSQL 14, 15, 16, 17. Each supported via a Cargo feature (`pg14` … `pg17`); only one is active at build time.
- Rust 1.90+ (edition 2024).
- pgrx 0.18+.
- Tree-sitter via the `tree-sitter` crate and per-language grammar crates fetched by Cargo.

## Open decisions

These are deferred to implementation, not invariants of the spec.

- Whether `moniker_pattern` is a compiled form or a text-glob.
- Variadic / array-form constructors (`graph_add_defs(code_graph, def_record[])`) for bulk loading.
- Per-language refinements of `bind_match`'s last-segment matching (callable bare-name, alias resolution). The default is byte-strict; refinements are layered as concrete cases motivate them.

## Non-scope

- **Stack-graphs / dynamic resolution.** The extension targets languages where the moniker is locally determinable.
- **Cross-project federation.** A moniker is scoped to a project via its authority. Federation between projects is an open question for another framework.
- **Concurrent collaborative editing of a synthetic `code_graph`.** The model assumes a single writer per value.
