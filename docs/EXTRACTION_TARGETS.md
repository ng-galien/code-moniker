# Extraction Targets

This document defines the extraction level `pg_code_moniker` must reach to
serve ESAC. The extension is not aiming for a toy extractor. ESAC already has
rich extractors; the native extension must preserve that semantic surface while
moving identity and graph storage into PostgreSQL-native `moniker` and
`code_graph` types.

## Purpose

`pg_code_moniker` owns stateless primitives:

- native symbol identity (`moniker`);
- per-module graph storage (`code_graph`);
- language extractors that turn source text plus an anchor into `code_graph`;
- constructors for symbolic and external graphs.

ESAC owns stateful concerns:

- repository configuration and srcset resolution;
- persistence in `esac.module`;
- projection into `esac.symbol` and `esac.linkage`;
- framework presets and archetype classification;
- coverage, tests, history and UI queries.

The extraction target is therefore: every supported language extractor must
emit enough defs, refs and metadata for ESAC to rebuild its existing symbol
and reference projections without losing current capabilities.

## Common Contract

Every supported language must emit a `code_graph` with the same conceptual row
families ESAC has today.

### Definitions

Each definition must carry:

- `moniker`: stable, content-addressed identity rooted under the srcset anchor.
- `kind`: shared vocabulary where possible (`module`, `class`, `interface`,
  `enum`, `type_alias`, `function`, `method`, `constructor`, `field`, `const`,
  `record`, `annotation_type`, `section`).
- `parent`: containment parent in the graph.
- `position`: byte range and line range when source-backed.
- `visibility`: language-specific but normalized to ESAC values (`public`,
  `protected`, `package`, `private`, `module`).
- `signature`: callable parameter signature or arity when needed for overload
  disambiguation.
- `type metadata`: return type, field type, qualified type name, and resolved
  type monikers when locally determinable.

### References

Each reference must carry:

- `source_moniker`: innermost enclosing definition, with module root for
  top-level refs.
- `target_moniker`: complete target identity when locally determinable.
- `target_name`: textual fallback when the target cannot be moniker-encoded.
- `kind`: shared vocabulary (`calls`, `method_call`, `reads`, `instantiates`,
  `extends`, `implements`, `uses_type`, `imports_module`, `imports_symbol`,
  `reexports`, `annotates`, `di_register`).
- `position`: byte/line location of the ref.
- `specifier`: import or external specifier when applicable.
- `confidence`: or enough metadata for ESAC projection to derive the current
  confidence levels (`resolved`, `scoped`, `name_match`, `local`,
  `unresolved`, `external`).
- `receiver hints`: chain and receiver moniker hints for method/field access
  resolution.

The extractor may emit unresolved/name-only refs, but those must be explicit.
They are not partial monikers.

### Deep Extraction

Languages that currently support ESAC deep extraction must keep it:

- parameters;
- local variables;
- inline callback/function-local defs;
- locals with source ranges and parent/function containment;
- enough density information for `outline`, `test`, and coverage attribution.

Deep-only symbols may remain outside repo-wide linkage projections, but they
must be available on demand from the graph.

### Determinism

Extraction must be deterministic for the same `(uri, source, anchor, presets,
extractor_version)` tuple:

- no table reads inside the extension;
- no dependency on extraction order;
- no backend-local ids in persisted identity;
- stable overload handling;
- stable reexport/import target monikers where source text contains enough
  information.

## Language Targets

### TypeScript / JavaScript / TSX / JSX

Status in ESAC: rich extractor, `grammed`, shared TSX grammar for
TS/JS/TSX/JSX, deep extraction available.

Target level: full current ESAC parity.

Definitions:

- modules;
- classes and interfaces;
- enums and type aliases;
- functions and methods;
- constructors;
- const/variable definitions, including arrow-function-as-variable where the
  value is callable;
- fields/properties when structurally available;
- section comments;
- deep-only parameters, locals, inline callbacks and function expressions.

References:

- named/default/namespace imports;
- relative imports resolved to module monikers;
- bare package imports represented as external/package refs;
- reexports, including barrel chains and aliases;
- function calls;
- method calls with receiver shape retained;
- instantiations (`new Foo`);
- `extends` / `implements`;
- type uses from annotations, generics, `keyof`, `typeof`, indexed access and
  nested type expressions;
- decorator refs as `annotates`;
- reads/identifier uses where ESAC currently emits them;
- DI registration calls (`di_register`) for known container idioms.

Resolution metadata:

- visibility derived from exports and class member modifiers;
- alias metadata for `import { X as Y }` and `export { X as Y }`;
- return type and field type monikers when determinable;
- receiver hints for chained calls (`foo().bar()`) and field access
  (`this.x.foo()`).

Non-target:

- full TypeScript type-checker semantics;
- arbitrary dynamic property resolution;
- runtime framework wiring beyond explicit framework-signal metadata.

### Java

Status in ESAC: rich extractor, `grammed`, no deep extractor registered yet.

Target level: full current ESAC parity plus graph-native output.

Definitions:

- modules/files;
- packages as path/context when needed by moniker construction;
- classes, interfaces, enums, records and annotation types;
- constructors;
- methods with parameter signatures for overload disambiguation;
- fields and constants;
- enum constants;
- section comments.

References:

- imports, including wildcard/package forms where represented today;
- calls to simple identifiers;
- method calls with receiver shape retained;
- instantiations;
- `extends` and `implements`;
- annotations;
- type uses from declarations, generics, arrays and qualified names;
- field/variable bindings sufficient to resolve receiver type hints.

Resolution metadata:

- Java visibility (`public`, `protected`, `package`, `private`);
- qualified names;
- type signatures;
- short and qualified return/field type names;
- return type and field type monikers when determinable;
- same-package lookup metadata;
- external type/package monikers for JDK/Maven dependencies.

Non-target:

- whole-program Java compiler resolution;
- reflection and string-based class loading;
- bytecode-only symbols unless represented by external graphs.

### Python

Status in ESAC: tree-sitter query extractor, `grammed`, no deep extractor
registered yet.

Target level: preserve current Python support and make it graph-native.

Definitions:

- modules;
- classes;
- functions;
- methods derived by containment under classes;
- section comments;
- future deep extraction for parameters and locals is desired, but not required
  for first parity unless ESAC registers a Python deep extractor.

References:

- `import X`, `import X as Y`, `import X.Y`;
- `from X import Y`, grouped imports and relative imports;
- plain function calls;
- method/attribute calls as `method_call`;
- inheritance bases;
- decorators;
- type annotations for parameters, returns and assignments;
- generic/subscript type uses where currently captured.

Resolution metadata:

- visibility by Python convention (`__x` private, `_x` module, dunder public);
- import specifiers preserving leading dots for relative imports;
- scoped, imported, name-match, local and external confidence derivation;
- receiver/type hints only when statically available.

Non-target:

- dynamic import resolution;
- monkey-patching/runtime attribute injection;
- full mypy/pyright type inference.

### SQL / PL/pgSQL

Status in ESAC: `grammed` dispatch backed by PostgreSQL/libpg_query parsing,
not tree-sitter.

Target level: preserve the libpg_query-backed extractor semantics.

Definitions:

- SQL files as modules;
- functions and procedures;
- trigger functions;
- tables as `class`;
- views as `interface`;
- overload-disambiguated functions where signatures are available.

References:

- schema-qualified and unqualified function calls at script top level;
- function calls inside PL/pgSQL bodies by parsing body expressions;
- references from pgTAP/test SQL into `esac.*` and other schema functions;
- best-effort table/view references if added by ESAC before migration.

Resolution metadata:

- schema and function name split;
- argument/signature data when available;
- robust handling of malformed or partial SQL by still emitting the module
  symbol and skipping broken AST sections.

Non-target:

- dynamic SQL inside `EXECUTE format(...)`;
- full SQL semantic analysis across search_path and temporary schema state.

## Next-Wave Languages

These languages are recognized as valuable for ESAC but are not current
`grammed` symbol-index languages in the checked ESAC registry. They should not
block the MVP. When added, they must satisfy the common contract above before
being considered supported.

### Rust

Minimum target:

- modules/files;
- structs, enums, traits, impl blocks and type aliases;
- functions, methods, associated functions and constants;
- `use` imports;
- calls and method calls when syntactically identifiable;
- trait impl relations;
- type uses in signatures;
- visibility (`pub`, crate/module-private);
- external crate/package monikers from `Cargo.toml` graphs.

### Go

Minimum target:

- packages and files;
- functions and methods;
- structs, interfaces, type aliases and constants;
- imports;
- calls, selector calls and instantiations/composite literals;
- interface implementation edges only when syntactically or cheaply inferable;
- exported/unexported visibility by capitalization;
- module/external monikers from `go.mod`.

### C# / C / C++ / PHP

Minimum target:

- file/module roots;
- top-level types/functions and methods;
- imports/includes/usings;
- call-like refs and type-use refs;
- visibility where the language exposes it;
- external package/library monikers where the project manifest provides them.

These are second-wave languages for `pg_code_moniker`; do not dilute the TS,
Java, Python and SQL targets to make room for them.

## Migration Bar

A language extractor is acceptable for ESAC only when all conditions hold:

- it can project to the existing `esac.symbol` shape without losing required
  columns;
- it can project to the existing `esac.symbol_ref` / future `esac.linkage`
  shape without losing ref kinds or target identity;
- `symbol_health`-style metrics do not regress materially against the legacy
  extractor on representative ESAC fixture repos;
- `find refs`, `outline`, `diagnostic`, `families/carriers`, test discovery and
  coverage reattachment continue to work;
- extraction is stateless and deterministic;
- unresolved cases are explicit and diagnosable.

For the first MVP, TypeScript/JavaScript parity is the acceptance gate. Java,
Python and SQL follow once the graph projection pipeline is proven.
