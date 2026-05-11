# code-moniker — specification

PostgreSQL extension. Native types for code symbol identity and code
graph storage, with indexed algebra. No table schemas, no triggers,
no I/O against external state. Pure types, operators, and per-language
extractors.

## Design constraints

- Symbol identity is a first-class PostgreSQL type with O(log n)
  indexed matching.
- A per-module code structure is a first-class PostgreSQL type,
  queryable with operators and indexes.
- Extractors are pure: same `(uri, source, anchor, presets)` ⇒ same
  `code_graph`. No table reads, no order dependence, no backend-local
  ids in persisted identity.
- Callers own state. Project anchors and presets are function
  arguments; persistence, schemas, and RLS belong to the caller.
- The moniker must be locally determinable from the source. Languages
  whose scoping requires whole-program resolution are not supported.

## Conceptual model

### The canonical tree

A program is a strict tree. Each node has exactly one parent. The
root is the project. The path from the root to a node is the node's
identity.

The tree has two regimes separated by an event-frontier called the
**srcset**:

| Regime | From | To | Kinds defined by |
|---|---|---|---|
| Project regime | root | srcset | caller (passed as project preset) |
| Language regime | srcset | leaves | language strategy (in extension) |

The srcset is the boundary node. Its kind is supplied by the caller
(`srcset`, `workspace_app`, etc.). Below it, the **first segment of
the language regime is `lang:<short>`**, posted by the extractor
(`lang:ts`, `lang:rs`, `lang:java`, `lang:python`, `lang:go`,
`lang:cs`, `lang:sql`). The extension does not interpret srcsets — it
receives the srcset moniker as an anchor and emits all source-derived
nodes under it, prefixed by the language segment.

External nodes (`external_pkg:...`) live entirely in the project
regime and have no language segment.

### The moniker

A node's identity in the canonical tree. Native PostgreSQL type with
operators and a custom GiST index.

Canonical external representation is a **typed-segment URI** using a
`+moniker` scheme profile:

```
<scheme>+moniker://<project>/<kind>:<name>[/<kind>:<name>...]
```

Every segment is `<kind>:<name>` separated by `/`. There is no
secondary separator. The base scheme is set via the GUC
`code_moniker.scheme` (default `code+moniker://`). Stored moniker
bytes are scheme-independent; only the text I/O consults the GUC.
The `+moniker` suffix identifies the canonical typed moniker profile,
not the final symbol kind — kinds are carried by each segment.

Examples (default scheme):

```
code+moniker://./lang:java/package:com/package:acme/package:domain/module:OrderService/class:OrderService/method:process(String)
code+moniker://./lang:ts/dir:src/dir:lib/module:user/class:UserService/method:findById(string)
code+moniker://./lang:python/package:acme/module:util/class:Helper/method:process(int)
code+moniker://./lang:rs/dir:src/dir:lang/dir:ts/module:mod/fn:parse(&str)
code+moniker://./external_pkg:maven/org.springframework/spring-core/6.1.0
```

The URI is symbolic only — it does not encode disk location. Source
location lives in a sidecar column (`source_uri`) on the holding row.

Names with reserved characters (`/`, `:`, `(`, `)`, backtick,
whitespace) are wrapped in backticks; literal backticks are doubled.

URI grammar and per-language path encoding: `moniker-uri.md`.

#### Operators

| Op            | Signature                                      | Semantics                                                                   |
|---------------|------------------------------------------------|-----------------------------------------------------------------------------|
| `=`           | `moniker = moniker → bool`                     | Byte-strict equality (every segment, including final kind). Total identity. |
| `?=`          | `moniker ?= moniker → bool`                    | `bind_match`: structural matching for cross-file linkage.                   |
| `<`, `<=`, `>`, `>=` | `moniker <op> moniker → bool`           | Byte-lex ordering (btree).                                                  |
| `<@`          | `moniker <@ moniker → bool`                    | Left is descendant of right.                                                |
| `@>`          | `moniker @> moniker → bool`                    | Left is ancestor of right; also `code_graph @> moniker`.                    |
| `\|\|`        | `moniker \|\| text → moniker`                  | Compose child from parent and a typed `kind:name` segment.                  |

`bind_match(left, right)` is true when:

- `left.project` is byte-equal to `right.project`;
- every segment of `left` and `right` except the last is byte-equal
  (which includes `srcset:` and `lang:` — cross-language matches are
  by design impossible);
- the **last** segment compares **name-only**:
  `left.last.name == right.last.name`. The last segment's `kind` may
  differ.

`bind_match` solves the problem that an extractor is local and does
not know the kind of a symbol it imports from another file. Imports
emit a placeholder last-segment kind (typically `path` or a
best-effort kind); the exporting module's def carries the true kind.
Byte-strict `=` would never match those two; `bind_match` does.

Default last-segment matching is byte-strict on the name.
Language-specific refinements (e.g. callable bare-name matching for
cases like `function:Y()` × `function:Y(int,str)`) are dispatched by
the shared `lang:` segment in `core/moniker/query.rs::last_segment_match`.
Current arms (`sql`, `ts`, `java`, `python`, `rs`, `go`) collapse on
`bare_callable_name`. Languages without an arm fall back to
byte-strict last-segment-name equality.

Accessors: `kind_of(moniker) → text`, `project_of(moniker) → text`,
`lang_of(moniker) → text` (the `lang:` segment payload, or empty for
project-regime monikers), `path_of(moniker) → text[]`,
`parent_of(moniker) → moniker`, `depth(moniker) → int`.

I/O: `moniker_in(cstring) → moniker`, `moniker_out(moniker) → cstring`.
URI parsing and serialization are part of the type's I/O contract.

Index: custom GiST opclass supporting `=`, `@>`, `<@`, `?=`. Btree
and hash opclasses also available for ordering, `DISTINCT`, and
hash joins. GIN over `moniker[]` indexes `graph_def_monikers` /
`graph_ref_targets` / `graph_export_monikers` /
`graph_import_targets`.

ODR (One Definition Rule) is a property the caller enforces with
`UNIQUE` constraints on `moniker` columns. The extension does not
police ODR — it provides the type whose equality makes ODR enforceable.

### The code_graph

A native type carrying the internal structure of a single module.

A `code_graph` contains:

- **Tree** — the intra-module containment hierarchy. Root is the
  module's own moniker. Children are types, members, nested
  functions, etc.
- **Defs** — for each node in the tree, a record carrying
  `(moniker, kind, visibility, signature, binding, origin, start_byte,
  end_byte)`. `start_byte` / `end_byte` are `int` byte offsets in
  source; `NULL` when the module has no source text (synthetic /
  external). The parent is implicit in the moniker chain.
- **Refs** — outgoing references. Each ref carries
  `(source, target, kind, receiver_hint, alias, confidence, binding,
  start_byte, end_byte)`. `source` is one of the module's own defs;
  `target` may be any moniker in the canonical tree, in any module.
  `kind` distinguishes the relation (`calls`, `imports_module`,
  `extends`, `uses_type`, …).

A `code_graph` is immutable as a value. Mutations are performed by
constructors that return a new value.

#### Binding semantics

Binding is the row-level qualifier for cross-file linkage. It is not
in the moniker bytes — it is a column on `DefRecord` and `RefRecord`.

`DefRecord.binding` ∈ {`export`, `local`, `none`, `inject`}:

| Value     | When                                                                                                |
|-----------|-----------------------------------------------------------------------------------------------------|
| `export`  | Symbol is addressable cross-module. `visibility` ∈ {`public`, `protected`, `package`} and `kind` ∉ {`local`, `param`, `section`}. Modules themselves are `export`. |
| `local`   | Symbol is module-scoped. `visibility` ∈ {`private`, `module`} or `kind` ∈ {`local`, `param`}.       |
| `inject`  | Symbol is a DI provider/target (e.g. `@Injectable`, `@Service`, `@Bean`, NestJS providers). Resolved by a container at runtime, not by static `import`. |
| `none`    | Concept does not apply (`kind = section`).                                                          |

`RefRecord.binding` ∈ {`import`, `local`, `none`, `inject`}:

| Value     | When                                                                                                |
|-----------|-----------------------------------------------------------------------------------------------------|
| `import`  | Ref points to another module via static import. `kind` ∈ {`imports_symbol`, `imports_module`, `reexports`}. The primary input to `bind_match`. |
| `local`   | Ref points inside the current module. Resolves via byte-strict `=` against the same module's defs.  |
| `inject`  | Ref demands a binding via DI container. `kind` ∈ {`di_register`, `di_require`}, plus constructor params whose annotated type is a known DI service. |
| `none`    | Ref's nature does not categorize as linkage (unresolved calls, reads of unknown identifiers).       |

For `bind_match` purposes there is no semantic distinction between
`import` and `inject`. The matching table is binary:

```
ref.binding ∈ {import, inject}  ×  def.binding ∈ {export, inject}
```

`inject` is a qualification for downstream traceability (which links
go through a DI container vs static import), not a matching axis.
Callers that want to filter DI separately project on `binding`.

#### Origin semantics

Origin is the row-level qualifier for **how a def came into existence**.
It is a column on `DefRecord`, opaque to `bind_match`.

`DefRecord.origin` ∈ {`extracted`, `declared`}:

| Value       | When                                                                                                                                              |
|-------------|---------------------------------------------------------------------------------------------------------------------------------------------------|
| `extracted` | Produced by an `extract_<lang>` call. Default for extractor output. Positions point into the source text.                                         |
| `declared`  | Produced by `code_graph_declare` from a declarative spec. The symbol exists at the moniker level only — no implementation. Positions are NULL.    |

Origin does not participate in `bind_match`. A `declared` def and an
`extracted` def with the same moniker resolve identically. When both
exist for one moniker, the caller applies its own precedence —
typically `extracted > declared`.

`RefRecord` has no `origin` column. A ref's provenance follows its
containing graph's defs.

#### Operators and functions

| Function | Signature | Semantics |
|---|---|---|
| `graph_root` | `code_graph → moniker` | The module's own moniker. |
| `graph_contains` | `code_graph @> moniker → bool` | Does this graph define this moniker? |
| `graph_defs` | `code_graph → setof DefRecord` | Iterate defs. |
| `graph_refs` | `code_graph → setof RefRecord` | Iterate refs. |
| `graph_locate` | `(code_graph, moniker) → TABLE(start_byte int, end_byte int)` | Position of a def in source. Empty if absent or no source. |
| `graph_def_monikers` | `code_graph → moniker[]` | Index helper: flatten defs to a sortable array. |
| `graph_ref_targets` | `code_graph → moniker[]` | Index helper: flatten outgoing ref targets. |
| `graph_export_monikers` | `code_graph → moniker[]` | Defs whose `binding` ∈ {`export`, `inject`}. |
| `graph_import_targets` | `code_graph → moniker[]` | Refs whose `binding` ∈ {`import`, `inject`}. |

Constructors:

| Function | Signature | Semantics |
|---|---|---|
| `graph_create` | `(root moniker, kind text) → code_graph` | New graph rooted at this moniker. |
| `graph_add_def` | `(graph, def moniker, kind text, parent moniker, start_byte int DEFAULT NULL, end_byte int DEFAULT NULL) → code_graph` | Add a def. `parent` must already be in the graph. |
| `graph_add_ref` | `(graph, source moniker, target moniker, kind text, start_byte int DEFAULT NULL, end_byte int DEFAULT NULL) → code_graph` | Add a ref. `source` must be a def in the graph. |
| `graph_add_defs` | `(graph, defs moniker[], kinds text[], parents moniker[]) → code_graph` | Bulk def insertion. Arrays are zipped position-wise. |
| `graph_add_refs` | `(graph, sources moniker[], targets moniker[], kinds text[]) → code_graph` | Bulk ref insertion. |
| `code_graph_declare` | `(spec jsonb) → code_graph` | Build a graph from a declarative specification. All defs carry `origin = declared`. |
| `code_graph_to_spec` | `(graph code_graph) → jsonb` | Reverse projection. Lossy on non-canonical ref kinds. `lang` is inferred from the root's `lang:` segment. |

Visibility, signature, binding, origin, receiver_hint, alias, and
confidence are produced by the extractors and surfaced by `graph_defs`
/ `graph_refs`. The point-wise `graph_add_def` / `graph_add_ref`
constructors take only the moniker-level fields; richer metadata
goes through `code_graph_declare` (a full JSONB spec) or comes out
of `extract_<lang>`.

Constructors return a new `code_graph`; they do not mutate.

### Per-language extraction

One function per supported language:

```
extract_<lang>(uri text, source text, anchor moniker, deep boolean DEFAULT false) → code_graph
```

`extract_typescript` takes one extra named argument
`di_register_callees text[] DEFAULT ARRAY[]::text[]` to declare which
factory-style calls should emit `di_register` refs.

Arguments:

- `uri` — disk path or symbolic identifier of the source. Used by
  the extractor to drive `dir:` / `package:` segments under the
  anchor.
- `source` — the source text.
- `anchor` — the srcset moniker under which all extracted defs are
  rooted. The extractor never produces monikers above this anchor.
- `deep` — when `true`, the extractor also emits parameters and
  local variables (`param:`, `local:` segments).

The extractor:

1. Parses `source` with the language's tree-sitter grammar (or the
   PG runtime parser for SQL).
2. Walks the AST.
3. Posts the `lang:<short>` segment as the first segment under
   `anchor`. Every produced moniker is rooted under
   `anchor/lang:<short>/...`.
4. Canonicalises each def and ref into a moniker rooted at `anchor`.
5. Tags `binding` on every def and ref per the rules in
   § Binding semantics.
6. Emits a `code_graph` value.

Supported languages: TypeScript / JavaScript / TSX / JSX, Rust, Java,
Python, Go, C#, PL/pgSQL.

`extract_<lang>` reads only its arguments. It does not look up other
modules, does not resolve refs across files. Cross-module resolution
is the caller's responsibility, performed by JOINing on `bind_match`
(cross-file) or `=` (intra-file or total identity).

#### Per-language extractor contract

Every supported language exposes a zero-sized type `Lang` implementing
the trait `lang::LangExtractor`. The trait formalises the contract
every extractor must satisfy and is the **single source of truth**
for what each language is allowed to emit.

```rust
pub trait LangExtractor {
    type Presets: Default;
    const LANG_TAG: &'static str;
    const ALLOWED_KINDS: &'static [&'static str];
    const ALLOWED_VISIBILITIES: &'static [&'static str];
    fn extract(uri: &str, source: &str, anchor: &Moniker, deep: bool,
               presets: &Self::Presets) -> CodeGraph;
}
```

`lang::assert_conformance::<E: LangExtractor>(graph, anchor)` validates
a produced graph against the contract. Each extractor's
`#[cfg(test)] extract_default` test helper invokes it on every
fixture. The invariants:

1. The graph's root is anchored under `anchor` and its first segment
   is `lang:<E::LANG_TAG>`.
2. Every def's `kind` is in `E::ALLOWED_KINDS` or in the universal
   internal vocabulary (`module`, `local`, `param`, `section`).
3. Every def's `kind` field byte-equals the kind of its moniker's
   last segment.
4. Every def's `visibility` is in `E::ALLOWED_VISIBILITIES` (or empty).
5. Every def's `origin` is `extracted` (extractors never produce
   `declared`).
6. Every ref's `binding` is consistent with its `kind` per
   § Binding semantics.
7. Every ref tagged `confidence = local` resolves to a def in the
   same graph via `bind_match`.

`code_graph_declare` consumes the same `ALLOWED_KINDS` /
`ALLOWED_VISIBILITIES` constants. The trait is the single source of
truth shared between extractor output validation and declarative spec
validation. `../declare_schema.json` mirrors these enumerations and
must be kept in sync with the trait constants.

#### Per-language import target shape

For `bind_match` to JOIN an import ref against the corresponding
export def, the import target's all-but-last segments must be
byte-equal to the def's all-but-last segments. Each extractor lowers
its language's import syntax to a target moniker that respects this:

- **TS / JS** — relative imports (`./foo`) inherit the importer's
  view (already in `lang:ts/`) and walk dirs as `path:`. Bare
  specifiers (`react`) land in the project regime under
  `external_pkg:`.
- **Python** — absolute project-local imports
  (`from acme.util import X`) build under
  `lang:python/package:.../module:.../path:X`. Relative imports
  (`from ._models import Response`) walk up the importer's module
  chain by `leading_dots - 1` package levels. Stdlib (`json`, `os`,
  …) keeps the project-regime `external_pkg:` shape.
- **Rust** — `crate::` builds under `lang:rs/`. The second-to-last
  piece becomes `module:<name>`, the last piece is `path:<symbol>`.
  `super::` / `self::` use the importer's view. Re-export chains
  (`use crate::a::b::c` where `c` is a `pub use` from a deeper
  module) cannot be resolved locally — the caller's projection layer
  follows the chain.
- **Java** — named imports (`import com.acme.Foo`) build under
  `lang:java/package:com/package:acme/module:Foo/path:Foo`. The last
  piece is duplicated as both `module:` (the file) and `path:` (the
  symbol) so `bind_match` unifies with `module:Foo/class:Foo`. JDK
  packages (`java.*`, `javax.*`) keep the `external_pkg:` shape.
- **Go** — imports (`import "github.com/x/y/z"`) lower to
  `external_pkg:<first piece>/path:<rest>` in the project regime
  (no `lang:go/` prefix on the import target). The default bind name
  is the path's last piece; `import alias "..."` overrides it;
  `import _ "..."` and `import . "..."` skip the bind.
- **C#** — `using A.B.C` lowers to `external_pkg:A/path:B/path:C` in
  the project regime. BCL prefixes (`System`, `Microsoft`,
  `mscorlib`) carry `confidence = external`. Aliases (`using X = ...`)
  and `static` / `global` flavours are surfaced via `RefAttrs`
  (`alias`, `binding`).
- **SQL / PL-pgSQL** — all `calls` refs are tagged `binding = local`
  (intra-module by language design) and use arity-only callable names
  like `function:bar(2)`, while defs use typed names like
  `function:bar(int4,text)`. `bind_match`'s `lang:sql` arm matches
  on the bare callable name, so cross-file linkage works without a
  separate refinement layer.

### Declarative graphs

A `code_graph` may be authored without source.
`code_graph_declare(spec jsonb) → code_graph` accepts a JSONB
specification of symbols and edges and emits a `code_graph`
indistinguishable in shape from extractor output. Every produced def
carries `origin = declared`.

Use cases:

- **Forward modeling** — declare a symbol before implementing it;
  callers see it in cross-file linkage immediately.
- **External libraries with no source** — declare the public surface
  so calls into them resolve via `bind_match`.
- **Architecture validation** — declare the intended graph and diff
  against actual extraction.

#### Canonical edge alphabet

Declarative edges use a canonical vocabulary aligned with the three
relations carried at the moniker level:

| Edge kind          | Maps to `REF_*` kind | Default binding                                              | Semantics                                  |
|--------------------|----------------------|--------------------------------------------------------------|--------------------------------------------|
| `depends_on`       | `imports_module`     | `import`                                                     | Source needs target at load time.          |
| `calls`            | `calls`              | `local` if `from` and `to` share the `module:` segment, else `none` | Source invokes target at runtime.   |
| `injects:provide`  | `di_register`        | `inject`                                                     | Source registers target in a DI container. |
| `injects:require`  | `di_require`         | `inject`                                                     | Source consumes target via DI container.   |

The richer extraction-time vocabulary (`uses_type`, `extends`,
`implements`, `reads`, `annotates`, `imports_symbol`, `reexports`,
`instantiates`, `method_call`) is not accepted by the declarative
constructor. Specs operate at the abstraction level of the canonical
model.

#### Spec format

A spec is a JSONB document validated against
`../declare_schema.json` (JSON Schema 2020-12). Top-level shape:

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

The shape is uniform across languages. Per-language profiles in the
schema restrict only `kind` and `visibility` enumerations:

| lang     | accepted `kind` values                                                                              | accepted `visibility` values            |
|----------|-----------------------------------------------------------------------------------------------------|-----------------------------------------|
| `ts`     | class, interface, type, function, method, const, enum, constructor, field, enum_constant, namespace | public, private, protected, module      |
| `rs`     | struct, enum, trait, impl, fn, method, const, static, type                                          | public, private, module                 |
| `java`   | class, interface, enum, record, annotation_type, method, constructor, field, enum_constant          | public, protected, package, private     |
| `python` | class, function, method, async_function                                                             | public, private, module                 |
| `go`     | type, struct, interface, func, method, var, const                                                   | public, module                          |
| `cs`     | class, interface, struct, record, enum, delegate, method, constructor, field, property, event       | public, protected, package, private     |
| `sql`    | function, procedure, view, table, schema                                                            | (visibility ignored)                    |

The visibility vocabulary is cross-language: `module` covers
package-private (Go), module-internal (Rust), nested-but-not-re-exported
(TS, Python). It maps to `VIS_MODULE` in the extractor output, so the
round-trip `extract → code_graph_to_spec → code_graph_declare` stays
valid for every supported language.

#### Validation

At ingest, `code_graph_declare`:

1. Validates `spec` against `../declare_schema.json` (structural shape
   + per-lang enum restrictions).
2. Parses each `MonikerURI` as a typed moniker.
3. Checks that `symbols[i].kind` matches the last segment kind of
   `symbols[i].moniker`.
4. Checks that `symbols[i].parent` is `root` or another declared symbol.
5. Rejects duplicate monikers in `symbols`.
6. Checks that `edges[i].from` references a declared symbol.
7. Builds the graph and returns it.

`edges[i].to` is not required to exist in the spec. The whole point
of declarative graphs is to reference symbols that may not exist yet;
`bind_match` resolves at query time against whatever defs are in the
corpus.

#### Reverse projection

`code_graph_to_spec(graph) → jsonb` walks a `code_graph` and emits a
JSONB document in the same shape as the input spec:

- The `lang` field is inferred from the root's `lang:` segment; if
  absent, the function errors.
- Every non-root def becomes a `symbols[i]` entry. `visibility` and
  `signature` are emitted only when non-empty. Positions and `origin`
  are dropped (they are not part of the spec format).
- Refs are filtered to the canonical edge alphabet:
  `imports_module → depends_on`, `calls → calls`,
  `di_register → injects:provide`,
  `di_require → injects:require`. All other ref kinds are silently
  dropped (`uses_type`, `extends`, `implements`, `reads`,
  `annotates`, `imports_symbol`, `reexports`, `instantiates`,
  `method_call`).

For any graph `g` produced by `code_graph_declare(s)`, the round-trip
`code_graph_declare(code_graph_to_spec(g))` produces a graph
equivalent to `g`. The function is lossy on extracted graphs whose
refs use non-canonical kinds; specs operate at a higher abstraction
level than extractor output.

#### Examples

Java — class + method with a call to a JDK dep:

```json
{
  "root": "code+moniker://app/srcset:main/lang:java/package:com/package:acme/module:UserService",
  "lang": "java",
  "symbols": [
    { "moniker":    "code+moniker://app/srcset:main/lang:java/package:com/package:acme/module:UserService/class:UserService",
      "kind":       "class",
      "parent":     "code+moniker://app/srcset:main/lang:java/package:com/package:acme/module:UserService",
      "visibility": "public" },
    { "moniker":    "code+moniker://app/srcset:main/lang:java/package:com/package:acme/module:UserService/class:UserService/method:findByEmail(String)",
      "kind":       "method",
      "parent":     "code+moniker://app/srcset:main/lang:java/package:com/package:acme/module:UserService/class:UserService",
      "visibility": "public",
      "signature":  "String" }
  ],
  "edges": [
    { "from": "code+moniker://app/srcset:main/lang:java/package:com/package:acme/module:UserService/class:UserService/method:findByEmail(String)",
      "kind": "calls",
      "to":   "code+moniker://app/external_pkg:java/path:util/path:Optional/path:empty" }
  ]
}
```

TypeScript:

```json
{
  "root": "code+moniker://app/srcset:main/lang:ts/dir:src/dir:services/module:user-service",
  "lang": "ts",
  "symbols": [
    { "moniker":    "code+moniker://app/srcset:main/lang:ts/dir:src/dir:services/module:user-service/class:UserService",
      "kind":       "class",
      "parent":     "code+moniker://app/srcset:main/lang:ts/dir:src/dir:services/module:user-service",
      "visibility": "public" }
  ],
  "edges": [
    { "from": "code+moniker://app/srcset:main/lang:ts/dir:src/dir:services/module:user-service/class:UserService",
      "kind": "depends_on",
      "to":   "code+moniker://app/external_pkg:typeorm/path:Repository" }
  ]
}
```

Rust — `pub fn` requiring a trait via DI wiring:

```json
{
  "root": "code+moniker://app/srcset:main/lang:rs/dir:domain/dir:user/module:service",
  "lang": "rs",
  "symbols": [
    { "moniker":    "code+moniker://app/srcset:main/lang:rs/dir:domain/dir:user/module:service/fn:create_user(String,String)",
      "kind":       "fn",
      "parent":     "code+moniker://app/srcset:main/lang:rs/dir:domain/dir:user/module:service",
      "visibility": "public",
      "signature":  "Result" }
  ],
  "edges": [
    { "from": "code+moniker://app/srcset:main/lang:rs/dir:domain/dir:user/module:service/fn:create_user(String,String)",
      "kind": "injects:require",
      "to":   "code+moniker://app/srcset:main/lang:rs/dir:infra/module:db/trait:UserRepo" }
  ]
}
```

## Storage and linkage

The extension defines no tables. It provides the types
(`moniker`, `code_graph`) and the operators; the caller chooses how
to persist and index. The pattern below is the recommended shape.

### Where a moniker belongs as a column

A `code_graph` already carries every moniker the module produces: its
own root via `graph_root(graph)`, its defs via
`graph_def_monikers(graph)`, its outgoing ref targets via
`graph_ref_targets(graph)`. Reproducing any of these as a separate
column on the holding row duplicates the graph's content and opens
the door to incoherence.

A `moniker` column is justified only when the moniker is an edge
endpoint that does not live inside any graph this row holds —
typically the `target_moniker` of a linkage edge, which by design
points across modules. Everywhere else the moniker is derived from
the graph at query time and indexed via expression indexes.

### One row per module

A module's structure is its `code_graph`. Source text is optional
(NULL for `declared` / `external` origins). The minimal table:

```sql
CREATE TABLE module (
    id          uuid PRIMARY KEY,
    graph       code_graph NOT NULL,
    source_text text,
    source_uri  text,
    origin      text NOT NULL  -- 'extracted' | 'declared' | 'external'
);
```

The module's identity is `graph_root(graph)`. Make it queryable and
unique with an expression index:

```sql
CREATE UNIQUE INDEX module_root_uniq ON module ((graph_root(graph)));
CREATE INDEX module_root_gist        ON module USING gist ((graph_root(graph)));
```

Updating a file produces a new `code_graph` and the row is rewritten
atomically.

### Linkage = JOIN on bind_match

Cross-file linkage is a single indexed JOIN. Use `bind_match` for
the cross-file case and `=` for total identity.

```sql
CREATE INDEX module_export_gin ON module USING gin (graph_export_monikers(graph));
CREATE INDEX module_import_gin ON module USING gin (graph_import_targets(graph));

SELECT m_def.id, m_imp.id
FROM module m_imp,
     LATERAL graph_refs(m_imp.graph) r,
     module m_def,
     LATERAL graph_defs(m_def.graph) d
WHERE r.binding IN ('import', 'inject')
  AND d.binding IN ('export', 'inject')
  AND bind_match(r.target, d.moniker);
```

The moniker GiST opclass indexes `bind_match`; lookups remain
O(log n) on the corpus.

### Containment queries

```sql
SELECT m.* FROM module m
WHERE graph_root(m.graph) <@ 'code+moniker://app/srcset:main'::moniker;

SELECT m.* FROM module m
WHERE graph_root(m.graph) <@ 'code+moniker://app/srcset:main/lang:java'::moniker;
```

Backed by `module_root_gist`.

### Linkage cache (optional)

For hot queries (`find-callers` at scale, deep call-graph traversal)
the linkage can be projected once into a flat table. This is the
table where storing monikers as columns is the right shape: each row
is a single edge whose endpoints are individual monikers, not
summaries of a graph.

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

INSERT INTO linkage (source_id, source_moniker, target_moniker, kind, binding, confidence, position)
SELECT m.id, r.source, r.target, r.kind, r.binding, r.confidence, r.position
FROM module m, LATERAL graph_refs(m.graph) AS r;
```

The table is reconstructible from `module` rows at any time; it is
a cache, not the truth. Truth lives in `module.graph`.

### Origins

The same `module` table holds all three origins uniformly.
Discriminate by `origin` when needed:

- `extracted` — `source_text` and `source_uri` non-NULL; positions
  in `graph` point into `source_text`; `graph_defs(graph)` rows have
  `origin = extracted`.
- `declared` — both NULL; positions in `graph` are NULL; produced by
  `code_graph_declare`; `graph_defs(graph)` rows have
  `origin = declared`.
- `external` — both NULL; `graph` may be minimal.

Promotion (`declared → extracted` when a source file appears) is an
UPDATE on the row that swaps `graph` and fills `source_*`. The
module's `id` is preserved.

## API surface

The SQL surface is generated by pgrx from `#[pg_extern]` and
`#[derive(PostgresType)]` annotations on Rust items.

- **Types**: `moniker`, `moniker_pattern`, `code_graph`.
- **Operators**:
  - `moniker = moniker → bool`
  - `bind_match(moniker, moniker) → bool`
  - `moniker <@ moniker → bool`, `moniker @> moniker → bool`
  - `moniker || (segment, kind) → moniker`
  - `moniker ~ moniker_pattern → bool`
  - `code_graph @> moniker → bool`
- **Accessors on `moniker`**: `kind_of`, `project_of`, `lang_of`,
  `path_of`, `parent_of`, `depth`.
- **Accessors / iterators on `code_graph`**: `graph_root`,
  `graph_defs`, `graph_refs`, `graph_locate`, `graph_def_monikers`,
  `graph_ref_targets`, `graph_export_monikers`, `graph_import_targets`.
- **Constructors**: `graph_create(moniker, kind) → code_graph`,
  `graph_add_def(...)`, `graph_add_ref(...)`,
  `code_graph_declare(jsonb) → code_graph`. Immutable: each returns
  a new `code_graph`.
- **Projection**: `code_graph_to_spec(code_graph) → jsonb`. Inverse
  of `code_graph_declare`, lossy on non-canonical ref kinds.
- **Extractors**: `extract_typescript(...)`, `extract_rust(...)`,
  `extract_java(...)`, `extract_python(...)`, `extract_go(...)`,
  `extract_csharp(...)`, `extract_plpgsql(...)`.
- **Index**: custom GiST opclass for `moniker` (`=`, `bind_match`,
  `<@`, `@>`, `~`); btree and hash opclasses for ordering and hash
  joins; GIN over `moniker[]`.

All functions are `IMMUTABLE STRICT PARALLEL SAFE` unless explicitly
noted.

## Compatibility

- PostgreSQL 14, 15, 16, 17. One major active at build time via the
  `pg14` … `pg17` Cargo feature.
- Rust 1.90+ (edition 2024).
- pgrx 0.18+.
- Tree-sitter via the `tree-sitter` crate and per-language grammar
  crates fetched by Cargo.
