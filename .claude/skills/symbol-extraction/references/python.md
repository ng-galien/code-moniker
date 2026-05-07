# Python

ESAC's Python extractor is a tree-sitter query extractor, with no current
repo-wide deep projection. The native target is to preserve current Python
support, make it graph-native, and keep resource-scoped locals/params
extractable. Source of truth: `docs/EXTRACTION_TARGETS.md` § Python.

Tree-sitter grammar: `tree-sitter-python`.

## Module shape

The Python module moniker is driven by the file path. Strip `.py`, then
split on `/`. There is no in-source package declaration; `__init__.py`
denotes a package directory but does not change the moniker shape — the
file itself is just `__init__`.

For namespace-package mode (no `__init__.py` chain), preset-driven: when
`presets.namespace_packages = true`, treat every directory under the anchor
as a package without requiring `__init__.py`. Otherwise stop the package
chain at the first directory without `__init__.py`.

## Definitions

Required:

- `module` (the file)
- `class_definition` → `class`
- top-level `function_definition` → `function` with full parameter
  type signature in the moniker (`function:make(int,str)`). Read the
  type from `typed_parameter` / `typed_default_parameter`; for
  parameters without an annotation, emit `_`
  (`function:make(int,_)`). The full type list is mirrored on
  `DefRecord.signature`. Arity-only is not acceptable.
- `function_definition` inside a `class_definition` body → `method`
  (containment-derived — the AST node is the same
  `function_definition`). Same signature rules; the implicit `self`
  parameter is excluded from the type list because it is not
  semantically a value argument.
- section comments (consistent with ESAC's existing Python extractor;
  follow whatever recognizer it uses)

Deep extraction of parameters and locals is in scope for resource-scoped
analysis even if ESAC does not project it into the repo-wide symbol index.
First parity may defer repo-wide projection, but the walker should capture
reliable `param` and `local` defs when it sees them. Parameters live in the
`parameters` child of `function_definition`, defaults in
`default_parameter`.

Methods are derived from containment, not from a separate node type. The
walker's job is to know whether the enclosing scope is a class body and
emit the right kind label accordingly.

## References

- `import_statement` → `imports_module`. `import X` → target module
  moniker, for example `.../module:X` when project-local or an explicit
  external module identity when package-local. `import X as Y` keeps the
  alias in metadata.
- `import_from_statement` → `imports_symbol`. `from X import Y` →
  `imports_symbol` with target module plus typed exported def segment when
  determinable, for example `.../module:X/function:Y()` or
  `.../module:X/class:Y`. Note the function target uses an empty
  parens shape because the imported symbol's signature isn't
  visible at the import site; the consumer matches against the
  exporting module's def whose moniker carries the full signature,
  using a name+arity projection. Grouped imports (`from X import
  a, b, c`) emit one ref per name. Relative imports (`from .foo
  import X`, `from ..bar import Y`) keep the leading dots in the
  import specifier metadata; resolve them against the importer's
  package the same way TS resolves `./` and `../`.
- plain `call` expression with an identifier callee → `calls`
- `call` expression with an `attribute` callee → `method_call`. Receiver
  shape retained.
- inheritance: `argument_list` of a `class_definition` → `extends` ref per
  base class (Python collapses extends/implements into base classes; emit
  `extends` for all bases).
- decorators on functions/classes/methods → `annotates`
- type annotations: `typed_parameter`, `typed_default_parameter`, return
  annotation, assignment with type annotation → `uses_type`
- generic/subscript type uses: `subscript` in a type position
  (`List[int]`, `dict[str, int]`) → `uses_type` for both base and
  parameter

## Resolution metadata

- visibility by Python convention:
  - dunder (`__x__`) → `public`
  - leading double underscore but not dunder (`__x`) → `private`
  - leading single underscore (`_x`) → `module` (module-private convention)
  - everything else → `public`
- import specifier preserves leading dots for relative imports — this is
  how the consumer reconstructs the relative path
- confidence derivation: `resolved` for locally determinable symbols,
  including imports that resolve to a known project symbol; `name_match`
  for names that match a local def; `local` for definitively local;
  `external` for import targets outside the project; `unresolved`
  otherwise. The extractor emits enough metadata for ESAC to derive these.
- receiver/type hints only when statically available (assignment with a
  type annotation, parameter type). Do not run inference.

## Non-targets

- dynamic import resolution (`importlib.import_module(name_var)`)
- monkey-patching, runtime attribute injection
- mypy/pyright-level type inference

## Edge cases worth a fixture

- decorators that are themselves expressions (`@functools.wraps(fn)`) →
  emit `annotates` against the resolvable base (`functools.wraps`)
- `if TYPE_CHECKING:` blocks — extract them like any other code; the
  consumer decides whether to demote `uses_type` refs sourced from such
  blocks
- `__all__` lists — not extracted as defs/refs; ESAC may project on them
  later, but the extractor emits regular module-level `const` defs only
- f-strings with embedded calls — descend into the formatted expressions
  and emit `calls` / `reads` like any expression context
