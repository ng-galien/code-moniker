# TypeScript / JavaScript / TSX / JSX

ESAC's reference extractor for TS/JS/TSX/JSX is the parity bar. The current
`src/lang/ts/` covers a subset; this page is what the extractor needs to grow
into. Source of truth: `docs/EXTRACTION_TARGETS.md` § TypeScript.

Tree-sitter grammar: `tree-sitter-typescript` (one grammar covering all four
flavours, modulo JSX elements).

## What's in today

Read `src/lang/ts/walker.rs` and `refs.rs` before adding anything. As of the
current state, the extractor emits:

- module def (root)
- `class_declaration` → class def + recursion into the class body
- `method_definition` inside a class body → method def
- top-level `function_declaration` → function def
- `import_statement` → one ref under the module root, with relative path
  resolution against the importer's directory
- `export_statement` is descended into for nested decls (e.g. `export class
  Foo {}`)

## What still has to land

Definitions:

- `interface_declaration` → emit `interface` def under module
- `enum_declaration` → emit `enum`; descend into members and emit each as
  `enum_constant` (Term)
- `type_alias_declaration` → emit `type_alias`
- `lexical_declaration` / `variable_declaration` for `const`/`let`/`var` →
  emit `const` defs at module scope; when the initializer is an
  `arrow_function` or `function_expression`, treat the binding as
  `function` (callable-as-variable). Capture both: the binding is a `const`
  with a callable initializer.
- `public_field_definition` / class properties → emit `field` defs under the
  class
- abstract class members: same shape, kind label stays `method`/`field`,
  carry an `abstract` modifier in metadata when the field exists
- section comments (`// ============ section ============` style, or
  whatever ESAC's existing extractor recognizes) → emit `section` defs

Constructors get the `constructor` kind label, not `method`. The structural
identity is a typed callable segment with arity/signature, for example
`#constructor:UserService(UserRepository)`.

References:

- named imports: `import { X } from './foo'` → `imports_symbol` ref with
  the symbol moniker as target. Extend the existing `handle_import` to also
  emit one ref per named specifier; the current code only emits the
  module-level ref.
- default imports: `import X from './foo'` → `imports_symbol` with target
  resolved to the target module's default-export identity when determinable.
  If the extractor cannot identify that export from local source, emit an
  explicit unresolved/name-only target and keep the alias in metadata. Do
  not encode default export as compact `#default` in canonical output.
- namespace imports: `import * as X from './foo'` → `imports_module` with
  alias.
- reexports: `export { X } from './foo'`, `export * from './foo'` →
  `reexports` kind. Barrel chains are common in TS — chase them only as far
  as the source text allows. Cross-file resolution stays in SQL.
- bare imports: `import 'react'` and `import React from 'react'` → external
  monikers. See `references/canonicalization.md` § "Bare imports".
- `call_expression` whose callee is an `identifier` → `calls` ref. Target
  moniker is the callee's resolved moniker when the binding is local;
  otherwise emit a name-only target with explicit unresolved kind.
- `call_expression` whose callee is a `member_expression` → `method_call`
  ref. Capture receiver hints: chain depth (`foo().bar()` is depth 2),
  receiver moniker hint when locally resolvable (`this.x.foo()` →
  `#class:Class#field:x#method:foo()`).
- `new_expression` → `instantiates` ref.
- `class_heritage` clauses → `extends` and `implements` refs from the class
  to each parent type.
- type uses: `type_annotation`, `type_arguments`, `keyof`, `typeof`,
  `indexed_access_type`, qualified `nested_type_identifier` →
  `uses_type` refs.
- decorators: `@Foo`, `@Foo(...)` → `annotates` refs.
- DI registrations: known container call patterns
  (`container.register(...)`, `container.bind(...)`, `useFactory:`) →
  `di_register` refs. Pattern list is caller-supplied via `presets`; do not
  hardcode.

Resolution metadata:

- visibility: derive from class member modifiers (`public`/`protected`/
  `private`) and from export keywords. Default for module top-level is
  `module` unless exported; class members default to `public`.
- alias metadata: keep both names for `import { X as Y }` and
  `export { X as Y }`. Today we have nowhere to store this — leave a TODO
  pointing at `RefRecord` extension.

## Deep extraction

ESAC's TS deep extractor registers parameters, locals, callbacks, and
function expressions. The native extension must keep that capability. The
shape:

- parameters of methods/functions → `param` defs under the enclosing
  callable, position is the parameter node range
- locals from `lexical_declaration` inside function bodies → `local` defs
- inline `arrow_function` / `function_expression` whose binding is a
  callback (e.g. `arr.map(x => x.id)`) → `function` defs anonymous-named
  by lexical position (`__cb_<line>_<col>`), parented under the enclosing
  callable

Deep-only symbols stay in the graph; ESAC's projection layer decides whether
to flatten them into `esac.symbol` or keep them graph-only. Keep them
deterministic — same source must produce the same anonymous names.

## Non-targets

- full TS type-checker semantics (no flow analysis, no inference of
  `unknown` → concrete type)
- arbitrary dynamic property resolution (`obj[name]` with computed name)
- runtime framework wiring (Next.js routes, Express decorators) beyond what
  `presets` declares as known patterns

## Test fixtures

Add fixtures under `tests/fixtures/ts/` once the inline tests outgrow
single-source strings — typically when an invariant spans more than ~30
lines of source. Keep fixtures minimal: one shape per fixture file, the
expected `code_graph` snapshot as a Rust constant in the test module.
