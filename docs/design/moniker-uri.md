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

The CLI accepts `--scheme <SCHEME>` to override the default
`code+moniker://` scheme.

## Project regime / Language regime

A moniker is split by an event-frontier into two regimes:

- **Project regime** — from the project root down to the srcset
  segment (`srcset:<name>`, `workspace_app:<name>`, …). Kinds are
  caller-supplied; the extension does not interpret them.
- **Language regime** — everything below the srcset segment,
  produced by an extractor.

The first segment of every language regime is `lang:<short>`, posted
by the extractor:

| Extractor          | Segment        | Path encoding              |
|--------------------|----------------|----------------------------|
| TypeScript         | `lang:ts`      | `dir:<seg>/module:<stem>`  |
| TSX                | `lang:tsx`     | `dir:<seg>/module:<stem>`  |
| JavaScript         | `lang:js`      | `dir:<seg>/module:<stem>`  |
| JSX                | `lang:jsx`     | `dir:<seg>/module:<stem>`  |
| Rust               | `lang:rs`      | `dir:<seg>/module:<stem>`  |
| Go                 | `lang:go`      | `dir:<seg>/module:<stem>`  |
| C#                 | `lang:cs`      | `dir:<seg>/module:<stem>`  |
| Java               | `lang:java`    | `package:<seg>/module:<stem>` |
| Python             | `lang:python`  | `package:<seg>/module:<stem>` |
| SQL / PL/pgSQL     | `lang:sql`     | `dir:<seg>/module:<stem>/schema:<name>` |

TypeScript, TSX, JavaScript, and JSX keep distinct language segments and rule
sections while sharing one package and linkage ecosystem.

`lang:` is mandatory for source-owned definitions. SDK and external
package targets have no `lang:` segment.

The `lang:` segment serves three purposes:

1. Co-locates multiple language regimes under a single srcset (a
   repo with Java service code and PL/pgSQL migrations under
   `srcset:main/lang:java/...` and `srcset:main/lang:sql/...`).
2. Anchors language-specific match strategies in `bind_match`.
3. Encodes language as identity: a `class:Foo` in Java and a
   `class:Foo` in TypeScript are not the same node.

## SDK and external package regimes

References outside the indexed source tree use two distinct canonical roots:

- `sdk:<lang>` identifies a language runtime, standard library, or platform
  SDK. Linkage records these references with origin `sdk`.
- `external_pkg:<package>` identifies a package or dependency outside the
  project. Its presence alone is not proof of a declared dependency: manifests,
  workspace package evidence, or language-specific build provenance determine
  whether linkage records `dependency` or `unknown_external`.

The SDK root carries provenance only. It does not replace lexical namespace
segments, so Java repeats `java` for the `java.lang` package:

```text
code+moniker://./sdk:java/path:java/path:lang/path:System/path:out/method:println(_)
```

Canonical SDK examples:

| Platform | Canonical target |
|----------|------------------|
| Java | `sdk:java/path:java/path:lang/path:String` |
| Rust | `sdk:rs/path:std/path:vec/struct:Vec` |
| Python | `sdk:python/path:builtins/path:str` |
| Go | `sdk:go/path:fmt/func:Println` |
| C | `sdk:c/path:libc/func:malloc` |
| C# | `sdk:cs/path:System/path:String` |
| TypeScript / Node | `sdk:ts/path:runtime/class:Promise`, `sdk:ts/path:node:fs` (`fs` and `node:fs`) |
| SQL / PostgreSQL | `sdk:sql/path:pg_catalog/path:now` |

These regimes are mutually exclusive: a canonical moniker never contains both
`sdk` and `external_pkg`. SDK references are external by design but are not
dependencies. Public audit totals preserve that distinction with `sdk`,
`dependency`, `injected_external`, and `unknown_external`; the compatibility
total `external` is their sum.

SDK ownership is conservative and follows language resolution precedence:

1. lexical and project-local bindings are considered before SDK fallback when
   the language permits shadowing;
2. a shared historical namespace is not sufficient ownership evidence
   (`javax.persistence` remains a Java dependency, and `System.Reactive`
   remains a C# dependency);
3. only context-independent provenance (`sdk` and `injected`) may be propagated
   between references. Dependency versus `unknown_external` is recomputed from
   the call-site manifest boundary.

The Java ownership table follows the Java SE/JDK 21 package surface for the
ambiguous `javax` namespace. The C# table is intentionally an exact,
allocation-free set of BCL namespaces and known CLR type paths; unknown
`System.*` namespaces remain manifest-owned. Until target versions and
reference packs become indexed inputs, false negatives are preferred over
silently converting a project dependency into SDK provenance.

## Binding lives outside the URI

The moniker is identity. Binding (whether a def is exported or
local, whether a ref is an import / DI injection / local) is **not**
in the moniker bytes — it lives as an explicit column on the def/ref
records. Keeping binding on the row lets the GiST opclass implement
`bind_match` as a purely structural operation, qualified by `WHERE`
predicates over binding columns at query time.

The current binding values are `export`, `local`, `import`, `inject`, and
`none`. Their producers are language-specific; structural URI matching remains
independent from that classification.

## Matching

The URI grammar feeds identity and hierarchy matching: byte-strict equality
on the full path, prefix containment for ancestor/descendant checks, and
cross-file binding where refs and defs share the same project, parent path,
and last segment name.

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
