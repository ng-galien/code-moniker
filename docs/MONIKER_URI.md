# Moniker URI Design

This document records the canonical URI design for `moniker`. It is the
durable persistence format. The byte representation is content-addressed
identity; this URI is its self-describing external form.

## Decision

The canonical external representation is typed per segment and uses a
`+moniker` scheme profile:

```text
<scheme>+moniker://<project>/<kind>:<name>[/<kind>:<name>...][#<kind>:<name>[#<kind>:<name>...]]
```

Examples:

```text
esac+moniker://app/srcset:main/lang:python/package:acme/module:util#class:UserService#method:findById(int)
esac+moniker://app/srcset:main/lang:java/package:com/package:acme/class:UserService#method:findById(String)
esac+moniker://app/srcset:main/lang:ts/dir:src/dir:lib/module:user#class:UserService#method:findById(string)
esac+moniker://app/external_pkg:npm/@types/node#module:fs#function:readFile()
```

Every segment has:

- `kind`: durable semantic kind, stored as text in the URI.
- `name`: segment label inside that kind.
- mandatory parameter type signature in the name payload of every
  callable segment (`method`, `function`, `constructor`, `operator`):
  `method:findById(String)`, `function:bar(int4,text)`. Same-name
  same-arity overloads with different parameter types must produce
  distinct moniker bytes; arity-only segments are forbidden in defs.
  The placeholder `_` (single underscore) fills slots where the source
  has no declared type (untyped JS, Python without hints, untyped
  Rust closure params).

The base scheme identifies the owning namespace (`esac`, `pcm`). The `+moniker`
suffix identifies the canonical typed moniker profile. The suffix must not
encode the final kind. A moniker is a heterogeneous path, so
`esac+class://...` is redundant and fragile.

## Project regime / Language regime

A moniker is split by an event-frontier into two regimes:

- **Project regime** — from the project root down to the srcset
  segment (`srcset:<name>`, `workspace_app:<name>`, …). Kinds are
  caller-supplied; the extension does not interpret them. External
  packages live entirely in the project regime
  (`external_pkg:maven/...`).
- **Language regime** — everything below the srcset segment, produced
  by an extractor.

The first segment of every language regime is **`lang:<short>`**, posted
by the extractor. Short language names are aligned with the
`src/lang/<lang>/` directory:

| Extractor          | Segment        |
|--------------------|----------------|
| TypeScript / JS    | `lang:ts`      |
| Rust               | `lang:rs`      |
| Java               | `lang:java`    |
| Python             | `lang:python`  |
| SQL / PL/pgSQL     | `lang:sql`     |

`lang:` is mandatory for every extractor-produced moniker. External
modules (no source) and project-regime nodes have no `lang:` segment.

The `lang:` segment serves three purposes:

1. Co-locates multiple language regimes under a single srcset (a repo
   with Java service code and PL/pgSQL migrations under
   `srcset:main/lang:java/...` and `srcset:main/lang:sql/...`).
2. Anchors language-specific match strategies in `bind_match` (see
   below) without leaking language into the project regime.
3. Encodes language as identity: a `class:Foo` in Java and a
   `class:Foo` in TypeScript are not the same node.

## Binding metadata

The moniker is identity. Binding (whether a def is exported or local,
whether a ref is an import / DI injection / local) is **not** in the
moniker bytes. It lives as an explicit column on the def/ref records:

- `DefRecord.binding` ∈ {`export`, `local`, `none`, `inject`}
- `RefRecord.binding` ∈ {`import`, `local`, `none`, `inject`}

Semantics are covered in `SPEC.md` § Binding semantics. Putting
binding on the row keeps the moniker pure identity (per the design
rule below) and lets the GiST opclass implement `bind_match` as a
purely structural operation, qualified by `WHERE` predicates over
binding columns at query time.

## Operators

Two equality operators, with distinct contracts.

### `=` — byte-strict equality

Equality of the canonical bytes, including the kind of every segment.
This is the matching primitive when total identity is required (ODR
enforcement, deduplication of two extractor outputs of the same
source, primary key constraints).

### `bind_match` — structural matching for cross-file linkage

`bind_match(left moniker, right moniker) → bool` is true when:

- `left.project == right.project` (byte-strict)
- every segment of `left` and `right` except the last is byte-equal
  (including `srcset:`, `lang:`, every parent segment)
- the **last** segment compares **name-only**: `left.last.name ==
  right.last.name`. The last segment's `kind` may differ.

The intent is to match an extractor's import-side ref against the
corresponding export-side def when the extractor has only partial
information about the target. Concretely:

```text
-- Python `from X import Y` produces a ref pointing at:
.../module:X/path:Y                 -- placeholder kind, the extractor
                                    -- doesn't know what Y is
-- The exporting module's def is:
.../module:X/function:Y(int,str)    -- typed callable

-- These are not equal byte-for-byte, but bind_match returns true:
--   project equal
--   .../module:X equal
--   last segment names equal: `Y` == `Y` (after stripping `(...)` from
--   the def-side name when comparing — the matching strategy is
--   language-specific and routed via the lang segment; the default
--   strategy is byte-strict name equality, refined per language as
--   needed)
```

The default match strategy is byte-strict on the last segment's name.
Language-specific refinements (callable bare-name matching, alias
resolution) are layered as needed and routed by reading the `lang:`
segment shared by both sides.

The matching primitive for cross-file linkage is `bind_match`, not
`=`. Consumers project linkage queries as:

```sql
SELECT m.* FROM module m, LATERAL graph_refs(m.graph) r
WHERE r.binding IN ('import', 'inject')
  AND EXISTS (
    SELECT 1 FROM module m2, LATERAL graph_defs(m2.graph) d
    WHERE d.binding IN ('export', 'inject')
      AND bind_match(r.target, d.moniker)
  );
```

`bind_match` is registered in the moniker GiST opclass with a
dedicated strategy number; index lookups are O(log n).

### Other operators

Containment (`<@`, `@>`), composition (`||`), and pattern matching
(`~`) all operate on canonical bytes and are unaffected by
`bind_match`. They remain byte-strict.

## Compact URI

A compact display profile keeps the base scheme without `+moniker` and
elides typing punctuation. Suitable for UI, logs, and human input as a
compatibility format:

```text
esac://app/main/python/acme/util#UserService#findById(int).
```

Compact form is lossy unless the caller supplies kind defaults; it is
not a persistence format.

Target SQL surface:

```sql
moniker_out(m)              -- canonical typed +moniker URI
moniker_compact(m)          -- compact human-readable URI
moniker_parse_compact(text) -- optional compatibility parser, preset-driven
```

## Segment Separators

The canonical path keeps the existing split between module path and
descriptor chain:

- `/` separates project / srcset / lang / module path segments;
- `#` separates symbol descriptors inside a module.

```text
-- TypeScript
esac+moniker://repo/srcset:main/lang:ts/dir:src/dir:lib/module:user#class:UserService#method:findById(string)

-- Java
esac+moniker://repo/srcset:main/lang:java/package:com/package:acme/class:UserService#constructor:UserService(String)

-- PL/pgSQL
esac+moniker://repo/srcset:db/lang:sql/schema:esac/module:plan#function:create_plan(uuid,text)

-- Symbolic planning before code exists (no extraction yet → no lang:)
esac+moniker://repo/workspace_app:api/module:billing#interface:PaymentGateway#method:charge(Money)

-- External package (no language regime)
esac+moniker://repo/external_pkg:maven/org.springframework/spring-core/6.1.0
```

## Source URI Is Separate

The moniker is symbolic identity. It is not a disk location.

`source_uri` remains a sidecar on the module row:

```text
moniker:    esac+moniker://repo/srcset:main/lang:java/package:com/package:acme/class:Foo
source_uri: src/main/java/com/acme/Foo.java
```

Consequences:

- moving a file changes `source_uri`, not necessarily `moniker`;
- multi-source-root disambiguation lives in the srcset segment;
- multiple language regimes coexist under one srcset via distinct
  `lang:` segments;
- symbolic modules can have monikers without source URIs;
- external modules can have monikers without local source.

## Escaping

Names with reserved characters are wrapped in backticks; literal
backticks are doubled inside escaped names.

```text
esac+moniker://repo/srcset:main/lang:ts/dir:`src/generated`/module:`weird:name`
```

Reserved characters: `/`, `#`, `:`, `(`, `)`, backtick, whitespace.

## Binary Encoding Requirement

The binary `moniker` representation must not rely on backend-local
kind ids for persisted identity.

Acceptable options:

- encode kind names directly in the moniker bytes (current implementation);
- encode stable, versioned kind ids from a built-in registry that is
  identical across extension versions, with migration rules;
- a hybrid format where common kinds are compact ids and custom kinds
  are stored as strings.

Backend-local interning is acceptable only as an in-memory cache after
parsing, not as stored identity.

## Design Rule

If a fact is required to preserve symbol identity, it belongs in the
moniker. If a fact qualifies a row's role in linkage (binding,
visibility, confidence), it belongs in the `code_graph` def/ref
records. If a fact is required to locate source text, render UI, or
classify framework semantics, it belongs in consumer tables, not in
the moniker.
