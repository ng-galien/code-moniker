# Moniker Canonicalization

Every extractor centralizes moniker construction in `canonicalize.rs`.
The target is the typed canonical form from `docs/MONIKER_URI.md`:

```text
esac+moniker://repo/srcset:main/dir:src/module:user#class:UserService#method:findById(String)
```

The compact form (`esac://repo/src/user#UserService#findById().`) is display
and compatibility only.

## Inputs

An extractor receives:

- `uri`: source locator, used for diagnostics and path-derived module shape.
- `source`: source text.
- `anchor`: the srcset/project-side moniker. The extractor never emits above
  this anchor.
- `presets`: caller-supplied layout/framework hints. No table reads.

## Module Moniker

Compute the module root once at the top of `extract`.

Rules:

- Start from `anchor`.
- Strip only the language's known source extension.
- Convert path/package/schema information into typed path segments.
- End with a `module:<name>` segment for source-backed modules unless the
  language target says otherwise.
- Keep `source_uri` separate. The moniker is symbolic identity, not the disk
  path.

TypeScript/JavaScript:

```text
uri:      src/lib/user.ts
anchor:   esac+moniker://repo/srcset:main
module:   esac+moniker://repo/srcset:main/dir:src/dir:lib/module:user
```

Java:

```text
uri:      src/main/java/com/acme/UserService.java
package:  com.acme
anchor:   esac+moniker://repo/srcset:main
module:   esac+moniker://repo/srcset:main/package:com/package:acme/module:UserService
```

Use the source package declaration as authoritative. Fall back to file path
only when the package is missing or malformed.

SQL/PLpgSQL:

```text
uri:      db/functions/plan/create_plan.sql
anchor:   esac+moniker://repo/srcset:db
module:   esac+moniker://repo/srcset:db/dir:functions/dir:plan/module:create_plan
```

Schema names belong in function/table monikers, not necessarily in the file
module moniker.

## Extending A Parent

Definitions are built by appending one typed segment to the parent:

```text
parent/class:Foo
parent/method:bar(String)
parent/field:repo
parent/param:id
parent/local:result
```

The on-the-wire URI uses `/` as the only segment separator across all
levels. The `#`-prefixed shape that appears in older docs is purely
conceptual — the actual `core::uri::serialize` uses `/` everywhere
and the moniker bytes themselves carry no separator distinction
(it's a flat list of `(kind, name)` pairs).

Callable segment names embed the **full parameter type signature**:

- `method:findById(String)`
- `function:bar(int4,text)`
- `function:make(_,_)` — JS without annotations or untyped Rust
  closure parameters use `_` placeholder
- `constructor:UserService(UserRepository)`

The same string is mirrored on `DefRecord.signature` for projection.
Arity-only segments (`bar(2)`) are forbidden — same-name same-arity
overloads with different parameter types are routine in every
supported language and must not collide.

Do not inline construction in walkers. Each language module exposes
its own helpers such as:

- `compute_module_moniker`
- `extend_segment` for path/context and term/type segments
- `extend_callable_typed` for callables with known parameter types
- `extend_callable_arity` for call sites where types are not statically
  known (raw_parser at SQL call sites, untyped JS calls). The
  resulting target moniker carries arity-only and the ref must be
  emitted with `confidence: unresolved` so consumers project
  on name+arity to match defs.

## External Monikers

External targets must be explicit:

```text
esac+moniker://repo/external_pkg:npm/lodash
esac+moniker://repo/external_pkg:maven/org.springframework/spring-core/6.1.0
esac+moniker://repo/external_pkg:jdk/java.base#class:String
```

Bare imports must not be encoded as ambiguous project-local path segments.
If the extractor cannot construct the final external target, emit explicit
external/unresolved metadata rather than a partial moniker.

## Anonymous Names

Anonymous callables are in scope for resource-scoped extraction. They need
deterministic names:

```text
__cb_<start_line>_<start_col>
```

Use source position, not visit-order counters. Document whether coordinates
are zero- or one-based in the language reference.

## Position

For defs, position should cover the declaration node. For refs, position
should cover the source expression/statement, not only the identifier.

Positions are used by outline, coverage attribution, local resource analysis
and planning workflows. Do not omit them for source-backed graphs.

## Anti-Patterns

- Building from a fresh project root instead of extending `anchor` or parent.
- Treating source path as identity after package/schema information is known.
- Using compact SCIP punctuation as the canonical form.
- Encoding semantic facts like visibility, confidence or framework archetype
  into the moniker. Those belong in graph metadata or ESAC tables.
- Producing target monikers that are "almost complete". Either produce a full
  moniker or an explicit unresolved/name-only ref.
