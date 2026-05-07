# Kind Vocabulary And URI Discipline

Kinds are part of persisted symbol identity. The canonical URI is typed per
segment:

```text
esac+moniker://repo/srcset:main/dir:src/module:user#class:UserService#method:findById(String)
```

The compact SCIP-like form (`esac://repo/src/user#UserService#findById().`)
is display/compatibility syntax only. Do not design extractor identity around
punctuation classes.

## Two Roles

Every extractor works with two related concepts:

- **Identity kind**: the segment kind written into the moniker
  (`module`, `class`, `method`, `local`, `param`, ...). It is durable and
  must survive serialization.
- **Semantic/ref kind**: the label on a definition or reference row
  (`class`, `uses_type`, `imports_symbol`, `di_register`, ...). It drives
  ESAC projection and queries.

For definitions these often match. For references, the ref kind labels the
edge, while the target moniker carries target segment kinds.

Transitional implementation may still expose `KindId` or `PunctClass`; treat
those as compact encoding/display concerns. They must not be the source of
truth for persisted identity.

## Shared Definition Vocabulary

Use these labels verbatim unless `docs/EXTRACTION_TARGETS.md` is updated.

- `module` — file/package module root.
- `srcset`, `srcset_main`, `workspace_app`, `workspace_lib` — project-side
  anchor kinds supplied by ESAC, not invented by the extractor.
- `dir`, `package`, `schema` — path/context segments.
- `class`, `interface`, `enum`, `type_alias`, `record`, `annotation_type`.
- `function`, `method`, `constructor`, `operator`.
- `field`, `const`, `enum_constant`.
- `param`, `local` — resource-scoped symbols. These are in-scope for
  extraction even when they are not projected into the repo-wide index.
- `section` — structural comments used by outline/exploration.

Language-specific additions (`trait`, `impl`, `package`, `namespace`, ...)
are spec changes. Update `docs/EXTRACTION_TARGETS.md` and this file before
using them.

## Shared Reference Vocabulary

Closed set for projection stability:

- `calls` — plain function call.
- `method_call` — receiver call (`obj.foo()`). Preserve receiver hints.
- `reads` — identifier read/use where the language target requires it.
- `instantiates` — constructor/class instantiation.
- `extends`, `implements`.
- `uses_type` — annotations, generics, qualified type names, `keyof`,
  `typeof`, indexed access, etc.
- `imports_module`, `imports_symbol`, `reexports`.
- `annotates` — decorators/annotations.
- `di_register` — explicit dependency-injection registration idioms.

Diagnostic conditions such as parse failures belong in graph metadata unless
the vocabulary is explicitly extended. Do not silently add ad hoc ref kinds.

## Canonical URI Examples

TypeScript:

```text
esac+moniker://repo/srcset:main/dir:src/module:user#class:UserService#method:findById(String)
esac+moniker://repo/srcset:main/dir:src/module:user#function:makeUser()
esac+moniker://repo/srcset:main/dir:src/module:user#function:makeUser()#local:repo
```

Java:

```text
esac+moniker://repo/srcset:main/package:com/package:acme/module:UserService#class:UserService#constructor:UserService(UserRepository)
esac+moniker://repo/srcset:main/package:com/package:acme/module:UserService#class:UserService#field:repo
```

Python:

```text
esac+moniker://repo/srcset:main/dir:pkg/module:service#class:UserService#method:find_by_id(id)
```

SQL:

```text
esac+moniker://repo/srcset:db/schema:esac/module:plan#function:create_plan(uuid,text)
```

## Signature And Overload Rule

Callables must encode enough signature information in the moniker to avoid
collisions:

- Java: parameter types are load-bearing.
- SQL/PLpgSQL: arity is required; parameter types are preferred.
- TypeScript/Python: arity/signature should be included when the source
  statically exposes it and collisions are possible.

Never disambiguate overloads with a visit-order counter. Use static signature
data or a deterministic source-position suffix for anonymous callables.

## Display Mapping

Compact display may map typed segments to SCIP-like punctuation:

- path/context segments → `/name`
- type-like segments → `#Name#`
- term-like segments → `#field.`
- callable segments → `#method().`

This mapping is lossy. It is allowed only for UI and compatibility input with
caller-supplied defaults.
