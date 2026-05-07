# Next-wave languages — Rust, Go, C#, C/C++, PHP

These are valuable for ESAC but are **not** current `grammed` symbol-index
languages in the checked ESAC registry. They must not block the MVP. Source
of truth: `docs/EXTRACTION_TARGETS.md` § Next-Wave Languages.

The migration bar is the same as for first-wave languages: the common
contract from `SKILL.md` (defs, refs, determinism). A next-wave extractor
lands only when ESAC has a path to project it into `esac.symbol` and
`esac.linkage` without losing required columns.

## Rust

Tree-sitter grammar: `tree-sitter-rust`.

Minimum target:

- modules / files (each `.rs` is a module; `mod foo;` declarations create
  child modules whose moniker is parent's path + `foo`)
- `struct_item`, `enum_item`, `trait_item`, `type_alias_item` →
  `class` / `enum` / `interface` / `type_alias`
- `impl_item` blocks — emit the impl as scope but its members get the
  parent struct/enum's moniker as parent. `impl Trait for Type` produces
  an `implements` ref from `Type` to `Trait`.
- `fn_item` → `function` at module scope, `method` inside an impl block
- associated `const_item` / `static_item` → `const`
- `use_declaration` → `imports_symbol` per imported name; group imports
  emit one ref per leaf
- calls and method calls when syntactically identifiable (no type
  inference); receiver shape retained for method calls
- visibility: `pub`, `pub(crate)`, `pub(super)`, `pub(in path)`,
  module-private. Map to `public` / `module` / `private` per ESAC's
  vocabulary; preserve the exact form in metadata.
- external crate / package monikers from `Cargo.toml` graphs (consumer
  passes the dependency graph in `presets`).

## Go

Tree-sitter grammar: `tree-sitter-go`.

Minimum target:

- packages and files; the package name is in the file's `package` clause
  and drives the module moniker (similar to Java)
- `function_declaration` → `function`, `method_declaration` → `method`
  with the receiver type as parent
- `type_declaration` for struct, interface, alias → `class` / `interface`
  / `type_alias`
- `const_declaration` / `var_declaration` at package scope → `const`
- imports: `import "path/to/pkg"` → `imports_module`. Aliased imports
  (`import alias "path"`) keep the alias.
- `call_expression` → `calls`
- `selector_expression` callee → `method_call`. Preserve receiver shape.
- composite literals (`Foo{}`) → `instantiates` against `Foo`'s type
- interface implementation edges only when **syntactically or cheaply
  inferable** (e.g. embedded interface). Do not run subtype inference.
- visibility: exported (capitalized first letter) → `public`; otherwise
  `package`. Mechanical, no language-keyword involved.
- module / external monikers from `go.mod` (consumer passes the module
  graph in `presets`).

## C# / C / C++ / PHP

Tree-sitter grammars: `tree-sitter-c-sharp`, `tree-sitter-c`,
`tree-sitter-cpp`, `tree-sitter-php`. (Verify availability before writing
the extractor — some grammars lag on dialect coverage.)

Minimum target — same shape across all four:

- file / module roots
- top-level types and functions
- methods inside types
- imports: `using` (C#), `#include` (C/C++), `use` (PHP),
  `using namespace` (C++)
- call-like refs and type-use refs
- visibility where the language exposes it (`public/private/protected/
  internal` in C#; access labels in C++; capitalization-and-`@` in PHP).
- external package / library monikers when the project manifest provides
  them (`csproj` for C#, vcpkg / conan for C++, `composer.json` for PHP).

These are second-wave. **Do not dilute** the TS / Java / Python / SQL
targets to make room for them. Each new language adds maintenance
overhead — earn its place by demonstrating that its contract is
satisfied on representative fixtures before merging.

## Common pitfalls when adding a next-wave language

- **Picking a stale tree-sitter grammar.** The Rust grammar is well
  maintained; Go's and C#'s are decent; the C++ grammar lags on modern
  dialects (concepts, modules); the C grammar is solid; the PHP grammar
  varies by maintainer fork. Check the grammar version vs the dialects
  the user actually has.
- **Treating impl/extension blocks as separate scopes.** Rust `impl`,
  C# extension methods, C++ free functions defined out-of-line — the
  member's parent is the type, not the syntactic block. Centralize this
  in `canonicalize.rs`.
- **Encoding visibility verbatim.** ESAC's vocabulary is closed.
  Translate to the closed set; keep the language-native form in metadata
  if the projection layer needs it.
- **Trying to resolve cross-file in the extractor.** Same rule as
  first-wave: the extension is stateless. Cross-file resolution is a SQL
  JOIN on the consumer side.
