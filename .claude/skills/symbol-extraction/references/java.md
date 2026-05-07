# Java

ESAC's Java extractor is rich but has no deep extractor registered. The
native target is full ESAC parity plus graph-native output. Source of truth:
`docs/EXTRACTION_TARGETS.md` § Java.

Tree-sitter grammar: `tree-sitter-java`.

## Module shape — packages matter

Unlike TS, the Java module moniker is driven by the **package declaration**
in the source, not just the file path. Use the file path only when the
package is missing (default package) or malformed.

```rust
// Pseudocode for compute_module_moniker
let pkg = parse_package_declaration(tree, source); // e.g. "com.acme.foo"
let class_stem = strip_java_extension(uri); // strip .java, take last segment
let mut b = MonikerBuilder::from_view(anchor.as_view());
for piece in pkg.split('.') {
    b.segment(package_kind, piece.as_bytes());
}
b.segment(module_kind, class_stem.as_bytes());
b.build()
```

Use typed `package` path segments for the `com.acme.foo` portion and a
typed `module` segment for the file-as-module segment. The canonical URI
shape is `.../package:com/package:acme/package:foo/module:MyClass`.
Compact display may omit those kinds, but the extractor must build the
canonical typed identity.

## Definitions

Required kinds:

- file-as-module
- packages as path/context (in the URI shape, not as separate def rows
  unless ESAC currently emits them)
- `class_declaration` → `class`
- `interface_declaration` → `interface`
- `enum_declaration` → `enum`; emit each enum constant as `enum_constant`
  (Term)
- `record_declaration` → `record` (Java 16+)
- `annotation_type_declaration` → `annotation_type`
- `constructor_declaration` → `constructor` with arity in the moniker
- `method_declaration` → `method`. Arity is **mandatory** for Java —
  overload disambiguation is the whole point. Compute from
  `formal_parameters`.
- `field_declaration` → `field` (Term). One def per declarator (`int a, b;`
  is two `field` defs).
- `enum_constant` for entries inside enum body
- section comments (Javadoc-style banners ESAC currently extracts)

## References

- import declarations: `import com.acme.Foo;` → `imports_symbol` (target is
  the FQN). Wildcard imports `import com.acme.*;` → `imports_module` with
  the package as target.
- static imports: `import static com.acme.Foo.bar;` → `imports_symbol`
  targeting the static member. Keep `static` flag in metadata.
- `method_invocation` with `name` field only → `calls` (the target is
  resolved against same-class, then same-package, then imports — emit the
  best moniker locally derivable; otherwise name-only with unresolved kind).
- `method_invocation` with `object` field → `method_call`. Receiver shape
  retained: identifier chain, `this.`, `super.`, qualified type names.
- `object_creation_expression` → `instantiates`
- `superclass` clause → `extends`
- `super_interfaces` clause → `implements`
- annotations on declarations → `annotates`
- type uses: `type_identifier` in declarations, generic `type_arguments`,
  array types, qualified `scoped_type_identifier` → `uses_type`
- field/variable bindings: keep enough info on the binding's def so receiver
  type hints can be resolved later (the field's static type is its type
  metadata)

## Resolution metadata

- visibility: `public`, `protected`, `package` (no modifier on a member),
  `private`. Default for top-level types is `package`. Read from `modifiers`
  child node.
- qualified names: the FQN is encoded in the moniker shape itself; nothing
  extra to store.
- type signatures: parameter types and return type as text; resolved
  monikers only when the type is locally determinable (same package, or
  imported with full FQN).
- short and qualified return/field type names: keep both. ESAC projects on
  the short form for find-by-type, on the qualified form for resolution.
- same-package lookup metadata: when a call's target is a same-package
  symbol, emit the resolved moniker. Cross-package targets without an
  import are unresolved.
- external monikers for JDK / Maven dependencies: when a type refers to
  something imported from `java.*`, `javax.*`, or a Maven coordinate
  declared in `presets`, emit an external moniker shape
  (`pcm+moniker://app/external_pkg:maven/group:org.springframework/artifact:spring-core/version:6.1.0#class:ApplicationContext`).
  Exact shape evolves with the Maven graph work; until then, name-only
  with explicit external kind is acceptable.

## Non-targets

- whole-program Java compiler resolution (no symbol table across files)
- reflection (`Class.forName`, dynamic proxy)
- bytecode-only symbols — those come from external graphs, not the
  extractor
- annotation processing semantics

## Migration bar

A Java extractor is acceptable when it can project to ESAC's
`esac.symbol` / `esac.linkage` shape with no required column lost. In
practice that means: visibility, signature text, return type text, short
type name, FQN, and overload-disambiguating arity must all be derivable
from the `code_graph`. Until `DefRecord` is extended to carry signature /
visibility, encode arity in the moniker and leave a TODO pointing at the
record extension.
