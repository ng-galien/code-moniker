# Moniker URI Design

This document records the target URI design for `moniker`. It supersedes the
early SCIP-like-only shape as the canonical persistence format.

## Decision

The canonical external representation is typed per segment and uses a
`+moniker` scheme profile:

```text
<scheme>+moniker://<project>/<kind>:<name>[/<kind>:<name>...][#<kind>:<name>[#<kind>:<name>...]]
```

Examples:

```text
esac+moniker://app/srcset:main/dir:src/module:util#class:UserService#method:findById(String)
esac+moniker://app/srcset:main/package:com/package:acme/class:UserService#method:findById(String)
esac+moniker://app/workspace_app:api/dir:src/module:router#function:registerRoutes()
esac+moniker://app/external_pkg:npm/@types/node#module:fs#function:readFile()
```

Every segment has:

- `kind`: durable semantic kind, stored as text in the URI.
- `name`: segment label inside that kind.
- mandatory parameter type signature in the name payload of every
  callable segment (`method`, `function`, `constructor`, `operator`):
  `method:findById(String)`, `function:bar(int4,text)`. Same-name
  same-arity overloads with different parameter types must produce
  distinct moniker bytes; arity-only segments are forbidden. The
  placeholder `_` (single underscore) fills slots where the source
  has no declared type (untyped JS, Python without hints, untyped
  Rust closure params).

The base scheme identifies the owning namespace (`esac`, `pcm`). The `+moniker`
suffix identifies the canonical typed moniker profile. The suffix must not
encode the final kind. A moniker is a heterogeneous path, so
`esac+class://...` is redundant and fragile.

The compact legacy/display profile keeps the base scheme without `+moniker`:

```text
esac://app/src/util#UserService#findById().
```

## Why Not SCIP-Like As Canonical?

The compact SCIP-like form is readable and familiar:

```text
esac://app/src/util#UserService#findById().
```

But punctuation only encodes broad classes:

- path;
- type;
- term;
- method.

ESAC needs durable distinctions that punctuation cannot represent:

- `srcset_main` vs `workspace_app` vs `dir` vs `module`;
- `class` vs `interface` vs `type_alias` vs `record`;
- `method` vs `constructor` vs `operator`;
- `external_pkg` vs `package` vs project path segment.

If those kinds live only in a backend-local registry or in out-of-band
metadata, the URI stops being a stable persistence format. The canonical URI
must be self-describing.

## Canonical vs Display

There are two useful string forms.

### Canonical URI

The canonical URI is the typed `+moniker` form. It is suitable for:

- persistence;
- equality/debugging across backends;
- import/export;
- logs and diagnostics where exact identity matters;
- text input/output of the PostgreSQL type.

Example:

```text
esac+moniker://app/srcset:main/dir:src/module:util#class:UserService#method:findById(String)
```

### Compact URI

The compact URI is the SCIP-like form under the base scheme. It is suitable for
humans, UI output and compatibility with ESAC's existing text monikers:

```text
esac://app/src/util#UserService#findById().
```

Compact form may be accepted as a compatibility input, but it is lossy unless
the caller supplies default kind mappings. It should not be the only persisted
truth.

Target SQL surface:

```sql
moniker_out(m)              -- canonical typed +moniker URI
moniker_compact(m)          -- compact human-readable URI
moniker_parse_compact(text) -- optional compatibility parser, preset-driven
```

## Segment Separators

The canonical path keeps the existing split between module path and descriptor
chain:

- `/` separates project/srcset/module path segments;
- `#` separates symbol descriptors inside a module.

This keeps display and migration easy while still typing every segment.

Examples:

```text
-- TypeScript
esac+moniker://repo/srcset:main/dir:src/dir:lib/module:user#class:UserService#method:findById(String)

-- Java
esac+moniker://repo/srcset:main/package:com/package:acme/class:UserService#constructor:UserService(String)

-- PL/pgSQL
esac+moniker://repo/srcset:db/schema:esac/module:plan#function:create_plan(uuid,text)

-- Symbolic planning before code exists
esac+moniker://repo/workspace_app:api/module:billing#interface:PaymentGateway#method:charge(Money)
```

## Source URI Is Separate

The moniker is symbolic identity. It is not a disk location.

`source_uri` remains a sidecar on the module row:

```text
moniker:    esac+moniker://repo/srcset:main/package:com/package:acme/class:Foo
source_uri: src/main/java/com/acme/Foo.java
```

Consequences:

- moving a file changes `source_uri`, not necessarily `moniker`;
- multi-source-root disambiguation lives in the srcset segment;
- symbolic modules can have monikers without source URIs;
- external modules can have monikers without local source.

## Escaping

Names with reserved characters must be escaped. The current backtick escaping
can be retained:

```text
esac+moniker://repo/srcset:main/dir:`src/generated`/module:`weird:name`
```

Reserved characters:

```text
/ # : ( ) ` whitespace
```

Literal backticks are doubled inside escaped names.

## Binary Encoding Requirement

The binary `moniker` representation must not rely on backend-local kind ids for
persisted identity.

Acceptable options:

- encode kind names directly in the moniker bytes;
- encode stable, versioned kind ids from a built-in registry that is identical
  across extension versions, with migration rules;
- use a hybrid format where common kinds are compact ids and custom kinds are
  stored as strings.

Backend-local interning is acceptable only as an in-memory cache after parsing,
not as stored identity.

## Compatibility Plan

Migration can be staged:

1. Keep parsing existing compact SCIP-like `esac://...` URIs.
2. Introduce canonical `esac+moniker://...` typed URI parsing and serialization.
3. Add `moniker_compact` for compact output.
4. Move `moniker_out` to canonical `+moniker` URI once ESAC projection code is ready.
5. Rebuild persisted text monikers in ESAC from legacy display form to typed
   canonical form.

During transition, equality must compare the canonical internal representation,
not the raw input string.

## Design Rule

If a fact is required to preserve symbol identity, it belongs in the moniker.
If a fact is required to locate source text, render UI, classify framework
semantics, or explain confidence, it belongs in `code_graph` metadata or ESAC
tables, not in the moniker.
