# Moniker URI

The canonical URI representation of a `moniker` value. The byte
representation is content-addressed identity; this URI is its
self-describing external form.

## Shape

```text
<scheme>+moniker://<project>/<kind>:<name>[/<kind>:<name>...]
```

Every segment is `<kind>:<name>` separated by `/`. There is no
secondary separator — module-internal symbols (classes, methods,
fields, parameters) are appended with `/`, the same as module-path
segments.

Examples:

```text
code+moniker://./lang:ts/dir:src/dir:lib/module:user/class:UserService/method:findById(string)
code+moniker://./lang:ts/dir:src/dir:lib/module:user/class:UserService/method:findById(string)/param:id
code+moniker://./lang:java/package:com/package:acme/package:domain/module:OrderService/class:OrderService/method:process(String)
code+moniker://./lang:python/package:acme/module:util/class:UserService/method:findById(int)
code+moniker://./lang:rs/dir:src/dir:lang/dir:ts/module:mod/fn:parse(&str)
```

Every segment has:

- `kind` — durable semantic kind, stored as text in the URI.
- `name` — segment label inside that kind.
- Callable segments (`method`, `function`, `fn`, `constructor`,
  `operator`) carry the parameter type signature in the name:
  `method:findById(String)`, `fn:parse(&str)`,
  `function:bar(int4,text)`. Same-name same-arity overloads with
  different parameter types produce distinct moniker bytes;
  arity-only segments are forbidden in defs. The placeholder `_`
  fills slots where the source has no declared type (untyped JS,
  Python without hints).

The base scheme identifies the owning namespace. The `+moniker`
suffix identifies the canonical typed moniker profile and must not
encode the final kind — a moniker is a heterogeneous path, so
`<base>+class://...` is redundant.

The scheme is configured via the Postgres GUC `code_moniker.scheme`
(default `code+moniker://`). The CLI accepts `--scheme <SCHEME>`
with the same default.

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

| Extractor          | Segment        | Path encoding              |
|--------------------|----------------|----------------------------|
| TypeScript / JS    | `lang:ts`      | `dir:<seg>/module:<stem>`  |
| Rust               | `lang:rs`      | `dir:<seg>/module:<stem>`  |
| Go                 | `lang:go`      | `dir:<seg>/module:<stem>`  |
| C#                 | `lang:cs`      | `dir:<seg>/module:<stem>`  |
| Java               | `lang:java`    | `package:<seg>/module:<stem>` |
| Python             | `lang:python`  | `package:<seg>/module:<stem>` |
| SQL / PL/pgSQL     | `lang:sql`     | `schema:<name>/module:<stem>` |

`lang:` is mandatory for every extractor-produced moniker. External
modules (no source) and project-regime nodes have no `lang:` segment.

The `lang:` segment serves three purposes:

1. Co-locates multiple language regimes under a single srcset (a
   repo with Java service code and PL/pgSQL migrations under
   `srcset:main/lang:java/...` and `srcset:main/lang:sql/...`).
2. Anchors language-specific match strategies in `bind_match`.
3. Encodes language as identity: a `class:Foo` in Java and a
   `class:Foo` in TypeScript are not the same node.

## Binding metadata

The moniker is identity. Binding (whether a def is exported or
local, whether a ref is an import / DI injection / local) is not in
the moniker bytes. It lives as an explicit column on the def/ref
records:

- `DefRecord.binding` ∈ {`export`, `local`, `none`, `inject`}
- `RefRecord.binding` ∈ {`import`, `local`, `none`, `inject`}

Semantics: `SPEC.md` § Binding semantics. Keeping binding on the row
lets the GiST opclass implement `bind_match` as a purely structural
operation, qualified by `WHERE` predicates over binding columns at
query time.

## Operators

### `=` — byte-strict equality

Equality of the canonical bytes, including the kind of every
segment. The matching primitive when total identity is required
(ODR enforcement, deduplication, primary keys).

### `?=` / `bind_match` — structural matching for cross-file linkage

`bind_match(left, right)` (operator `?=`) is true when:

- `left.project == right.project` (byte-strict);
- every segment except the last is byte-equal (including `srcset:`,
  `lang:`, every parent segment);
- the **last** segment compares **name-only**:
  `left.last.name == right.last.name`. The last segment's `kind`
  may differ.

The intent is to match an extractor's import-side ref against the
corresponding export-side def when the extractor has only partial
information about the target. For Python `from X import Y`:

```text
-- Import-side ref (extractor doesn't know what Y is):
.../module:X/path:Y
-- Export-side def:
.../module:X/function:Y(int,str)
```

These are not equal byte-for-byte, but `?=` returns true: project
equal, `.../module:X` equal, last segment names equal (`Y == Y`
after stripping `(...)` from the def-side name per the `lang:python`
arm).

Default last-segment matching is byte-strict on the name.
Language-specific refinements (callable bare-name matching) are
dispatched by the shared `lang:` segment. `bind_match` is registered
in the moniker GiST opclass; index lookups are O(log n).

### Containment and composition

`<@` / `@>` test byte-prefix containment. `||` composes a child
moniker from a parent and a typed segment:

```sql
'code+moniker://app/srcset:main'::moniker || 'lang:ts'   -- compose by typed segment
compose_child(parent, kind text, name text)              -- compose by (kind, name) pair
```

These operators are byte-strict and unaffected by `bind_match`.

## Compact URI

`moniker_compact(m)` produces a display form without the `+moniker`
suffix. It is lossy and not a persistence format. `match_compact(m,
compact text)` checks a compact string against a binary moniker.

## Source URI is separate

The moniker is symbolic identity. It is not a disk location.
`source_uri` is a sidecar on the holding row:

```text
moniker:    code+moniker://./lang:java/package:com/package:acme/module:Foo/class:Foo
source_uri: src/main/java/com/acme/Foo.java
```

Consequences:

- Moving a file changes `source_uri`, not necessarily `moniker`
  (the moniker still contains the file stem).
- Multi-source-root disambiguation lives in the `srcset:` segment.
- Multiple language regimes coexist under one srcset via distinct
  `lang:` segments.
- Symbolic and external modules have monikers without source URIs.

## Escaping

Names with reserved characters are wrapped in backticks; literal
backticks are doubled inside escaped names.

```text
code+moniker://repo/lang:ts/dir:`src/generated`/module:`weird:name`
```

Reserved characters: `/`, `:`, `(`, `)`, backtick, whitespace.

## Text form is transport, not a manipulation API

The text form returned by `moniker_out(m)` is a self-describing
transport encoding. It is **not** a stable surface for caller regex
or string manipulation. Callable name suffixes can contain spaces,
pipes, slashes, and arrows from type annotations
(`f((x: number) => string)`, `f(string | null)`); the serializer
backtick-wraps such names and doubles literal backticks inside them.
Stripping a `(...)` suffix with a hand-rolled regex is unsafe — it
can leave backtick quoting unbalanced and break the round-trip.

Callers should not re-parse `m::text`. The supported surface:

- `?=` (`bind_match`) — symbol equivalence, dispatched per-language.
- `bare_callable_name(m) → moniker` — strips the parens-and-after
  suffix from the last segment's name.
- `kind_of(m)`, `project_of(m)`, `lang_of(m)`, `path_of(m)`,
  `parent_of(m)`, `depth(m)` — typed accessors over the binary form.

## Design rule

- A fact required to preserve symbol identity belongs in the moniker.
- A fact that qualifies a row's role in linkage (binding, visibility,
  confidence) belongs in the `code_graph` def/ref records.
- A fact required to locate source text, render UI, or classify
  framework semantics belongs in caller tables, not in the moniker.
