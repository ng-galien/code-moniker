# Moniker URI

The canonical URI representation of a `moniker` value. The byte
representation is content-addressed identity; this URI is its
self-describing external form.

## Shape

The URI is typed per segment and uses a `+moniker` scheme profile:

```text
<scheme>+moniker://<project>/<kind>:<name>[/<kind>:<name>...][#<kind>:<name>[#<kind>:<name>...]]
```

Examples:

```text
code+moniker://app/srcset:main/lang:python/package:acme/module:util#class:UserService#method:findById(int)
code+moniker://app/srcset:main/lang:java/package:com/package:acme/class:UserService#method:findById(String)
code+moniker://app/srcset:main/lang:ts/dir:src/dir:lib/module:user#class:UserService#method:findById(string)
code+moniker://app/external_pkg:npm/@types/node#module:fs#function:readFile()
```

Every segment has:

- `kind` — durable semantic kind, stored as text in the URI.
- `name` — segment label inside that kind.
- A mandatory parameter type signature in the name payload of every
  callable segment (`method`, `function`, `constructor`, `operator`):
  `method:findById(String)`, `function:bar(int4,text)`. Same-name
  same-arity overloads with different parameter types must produce
  distinct moniker bytes; arity-only segments are forbidden in defs.
  The placeholder `_` (single underscore) fills slots where the source
  has no declared type (untyped JS, Python without hints, untyped
  Rust closure params).

The base scheme identifies the owning namespace. The `+moniker`
suffix identifies the canonical typed moniker profile and must not
encode the final kind — a moniker is a heterogeneous path, so
`<base>+class://...` is redundant and fragile.

The full scheme (`<base>+moniker://`) is configured via the Postgres
GUC `code_moniker.scheme`. Default `code+moniker://`. A consumer
database sets its own once:

```sql
ALTER DATABASE myapp SET code_moniker.scheme = 'myapp+moniker://';
```

## Project regime / Language regime

A moniker is split by an event-frontier into two regimes:

- **Project regime** — from the project root down to the srcset
  segment (`srcset:<name>`, `workspace_app:<name>`, …). Kinds are
  caller-supplied; the extension does not interpret them. External
  packages live entirely in the project regime
  (`external_pkg:maven/...`).
- **Language regime** — everything below the srcset segment,
  produced by an extractor.

The first segment of every language regime is `lang:<short>`, posted
by the extractor:

| Extractor          | Segment        |
|--------------------|----------------|
| TypeScript / JS    | `lang:ts`      |
| Rust               | `lang:rs`      |
| Java               | `lang:java`    |
| Python             | `lang:python`  |
| Go                 | `lang:go`      |
| C#                 | `lang:cs`      |
| SQL / PL/pgSQL     | `lang:sql`     |

`lang:` is mandatory for every extractor-produced moniker. External
modules (no source) and project-regime nodes have no `lang:` segment.

The `lang:` segment serves three purposes:

1. Co-locates multiple language regimes under a single srcset (a repo
   with Java service code and PL/pgSQL migrations under
   `srcset:main/lang:java/...` and `srcset:main/lang:sql/...`).
2. Anchors language-specific match strategies in `bind_match` without
   leaking language into the project regime.
3. Encodes language as identity: a `class:Foo` in Java and a
   `class:Foo` in TypeScript are not the same node.

## Binding metadata

The moniker is identity. Binding (whether a def is exported or local,
whether a ref is an import / DI injection / local) is not in the
moniker bytes. It lives as an explicit column on the def/ref records:

- `DefRecord.binding` ∈ {`export`, `local`, `none`, `inject`}
- `RefRecord.binding` ∈ {`import`, `local`, `none`, `inject`}

Semantics: `SPEC.md` (this directory) § Binding semantics. Keeping
binding on the row lets the GiST opclass implement `bind_match` as
a purely structural operation, qualified by `WHERE` predicates over
binding columns at query time.

## Operators

Two equality operators with distinct contracts.

### `=` — byte-strict equality

Equality of the canonical bytes, including the kind of every segment.
The matching primitive when total identity is required (ODR
enforcement, deduplication of two extractor outputs of the same
source, primary key constraints).

### `bind_match` — structural matching for cross-file linkage

`bind_match(left moniker, right moniker) → bool` is true when:

- `left.project == right.project` (byte-strict);
- every segment of `left` and `right` except the last is byte-equal
  (including `srcset:`, `lang:`, every parent segment);
- the **last** segment compares **name-only**:
  `left.last.name == right.last.name`. The last segment's `kind` may
  differ.

The intent is to match an extractor's import-side ref against the
corresponding export-side def when the extractor has only partial
information about the target. For Python `from X import Y`:

```text
-- Import-side ref (extractor doesn't know what Y is):
.../module:X/path:Y
-- Export-side def:
.../module:X/function:Y(int,str)
```

These are not equal byte-for-byte, but `bind_match` returns true:
project equal, `.../module:X` equal, last segment names equal
(`Y == Y` after stripping `(...)` from the def-side name per the
`lang:python` arm).

Default last-segment matching is byte-strict on the name.
Language-specific refinements (callable bare-name matching, alias
resolution) are dispatched by the shared `lang:` segment.

`bind_match` is registered in the moniker GiST opclass with a
dedicated strategy number; index lookups are O(log n).

### Other operators

Containment (`<@`, `@>`), composition (`||`), and pattern matching
(`~`) operate on canonical bytes and are unaffected by `bind_match`.
They remain byte-strict.

## Compact URI

A compact display profile keeps the base scheme without `+moniker`
and elides typing punctuation. Suitable for UI and logs:

```text
code://app/main/python/acme/util#UserService#findById(int)
```

Compact form is lossy unless the caller supplies kind defaults; it
is not a persistence format.

SQL surface:

```sql
moniker_out(m)              -- canonical typed +moniker URI
moniker_compact(m)          -- compact human-readable URI
moniker_parse_compact(text) -- optional compatibility parser, preset-driven
```

## Segment separators

- `/` separates project / srcset / lang / module path segments.
- `#` separates symbol descriptors inside a module.

```text
-- TypeScript
code+moniker://repo/srcset:main/lang:ts/dir:src/dir:lib/module:user#class:UserService#method:findById(string)

-- Java
code+moniker://repo/srcset:main/lang:java/package:com/package:acme/class:UserService#constructor:UserService(String)

-- PL/pgSQL
code+moniker://repo/srcset:db/lang:sql/schema:public/module:plan#function:create_plan(uuid,text)

-- Symbolic planning before code exists (no extraction yet → no lang:)
code+moniker://repo/workspace_app:api/module:billing#interface:PaymentGateway#method:charge(Money)

-- External package (no language regime)
code+moniker://repo/external_pkg:maven/org.springframework/spring-core/6.1.0
```

## Source URI is separate

The moniker is symbolic identity. It is not a disk location.
`source_uri` is a sidecar on the holding row:

```text
moniker:    code+moniker://repo/srcset:main/lang:java/package:com/package:acme/class:Foo
source_uri: src/main/java/com/acme/Foo.java
```

Consequences:

- Moving a file changes `source_uri`, not necessarily `moniker`.
- Multi-source-root disambiguation lives in the `srcset:` segment.
- Multiple language regimes coexist under one srcset via distinct
  `lang:` segments.
- Symbolic modules can have monikers without source URIs.
- External modules can have monikers without local source.

## Escaping

Names with reserved characters are wrapped in backticks; literal
backticks are doubled inside escaped names.

```text
code+moniker://repo/srcset:main/lang:ts/dir:`src/generated`/module:`weird:name`
```

Reserved characters: `/`, `#`, `:`, `(`, `)`, backtick, whitespace.

## Text form is transport, not a manipulation API

The text form returned by `moniker_out(m)` (i.e. `m::text`) is a
self-describing transport encoding. It is **not** a stable surface
for caller regex or string manipulation. Callable name suffixes can
contain spaces, pipes, slashes, and arrows from type annotations
(`f((x: number) => string)`, `f(string | null)`); the serializer
backtick-wraps such names and doubles literal backticks inside them.
Stripping a `(...)` suffix with a hand-rolled regex is unsafe — it
can leave backtick quoting unbalanced and break the round-trip.

Callers should never re-parse `m::text`. The supported surface:

- `?=` (`bind_match`) — symbol equivalence, dispatched per-language.
  Use this for cross-file linkage queries instead of derived columns.
- `bare_callable_name(m moniker) → moniker` — strips the parens-and-
  after suffix from the last segment's name. Useful when a
  denormalized "stripped" column is needed for indexing.
- `kind_of(m)`, `path_of(m)`, `parent_of(m)`, `project_of(m)`,
  `lang_of(m)`, `depth(m)` — typed accessors over the binary form.

If a manipulation is missing from this list, the right answer is a
new typed accessor, not `regexp_replace` on the text form.

## Binary encoding requirement

The binary `moniker` representation must not rely on backend-local
kind ids for persisted identity. Acceptable options:

- Encode kind names directly in the moniker bytes (current implementation).
- Encode stable, versioned kind ids from a built-in registry that is
  identical across extension versions, with migration rules.
- A hybrid format where common kinds are compact ids and custom
  kinds are stored as strings.

Backend-local interning is acceptable only as an in-memory cache
after parsing, not as stored identity.

## Design rule

- If a fact is required to preserve symbol identity, it belongs in
  the moniker.
- If a fact qualifies a row's role in linkage (binding, visibility,
  confidence), it belongs in the `code_graph` def/ref records.
- If a fact is required to locate source text, render UI, or
  classify framework semantics, it belongs in caller tables, not in
  the moniker.
