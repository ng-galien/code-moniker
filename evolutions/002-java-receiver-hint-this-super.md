# 002 — java extractor : `this` / `super` not tagged in `receiver_hint`

## Diagnostic

The Java extractor never emits `b"this"` or `b"super"` as
`receiver_hint`. Calls of the form `this.foo()` or `super.foo()` are
classified as `receiver_hint='identifier'` (the receiver expression is
read as a regular identifier, the `this`/`super` keyword case is not
distinguished).

Reference dataset : repo `rsql-jpa-specification`, 76 modules, 9 569
refs.

`receiver_hint` distribution on `kind='method_call'` (n = 2 856) :

| receiver_hint | n |
|---|---|
| `identifier` | 2 167 |
| `call` | 659 |
| (empty) | 21 |
| `member` | 9 |
| **`this`** | **0** |
| **`super`** | **0** |

By contrast, the TS extractor emits `this` / `super` correctly (esac
v2 dogfood : 9 method_calls resolved via the consumer-side
`this`/`super` walk on `parent_of(source_moniker)`).

## Code reference

`src/lang/ts/refs.rs:424-437` (`receiver_hint` for TS) :

```rust
fn receiver_hint(member_expr: Node<'_>) -> &'static [u8] {
    let Some(obj) = member_expr.child_by_field_name("object") else {
        return b"";
    };
    match obj.kind() {
        "this" => b"this",
        "super" => b"super",
        "identifier" => b"identifier",
        "call_expression" => b"call",
        "member_expression" => b"member",
        "subscript_expression" => b"subscript",
        _ => b"",
    }
}
```

The Java equivalent (`src/lang/java/refs.rs`) does not surface this
distinction.

## Impact

`this.foo()` / `super.foo()` are emitted with target rooted at
`self.module`, receiver_hint=`identifier`. The consumer cannot
distinguish them from `someVar.foo()` (where `someVar` is a regular
local) and therefore cannot trigger the `this`/`super` walk to the
enclosing class def. All such calls land in `name_match` orphans.

Sample (`rsql-common/src/main/java/io/github/perplexhub/rsql/PathUtils.java`) :

```java
public class PathUtils {
    public static Optional<String> findMappingOnWhole(String path, Map<String,String> map) {
        return findMappingOnBeginning(path, map);   // intra-class call
    }
    public static Optional<String> findMappingOnBeginning(...) { ... }
}
```

`findMappingOnBeginning` from inside `findMappingOnWhole` produces a
method_call ref with `receiver_hint=''` (empty) and target rooted at
the caller's module — no link to the class def of
`findMappingOnBeginning` in the same class.
