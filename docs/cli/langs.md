# `code-moniker langs` and `shapes`

Two discovery commands. They take no path and produce no side effect — they print the vocabulary the CLI recognises. Useful for building `--kind` / `--shape` filters on [extract](extract.md) or for writing rules with the [rule DSL](check-dsl.md).

## `langs`

```
code-moniker langs [TAG] [--format text|json]
```

| Form                       | Output                                                  |
| -------------------------- | ------------------------------------------------------- |
| `code-moniker langs`       | every supported tag, one per line                       |
| `code-moniker langs <TAG>` | kinds grouped by shape + accepted visibilities for that lang |

Supported tags:

| Tag      | Extensions                                |
| -------- | ----------------------------------------- |
| `ts`     | `.ts` `.tsx` `.js` `.jsx` `.mjs` `.cjs`   |
| `rs`     | `.rs`                                     |
| `java`   | `.java`                                   |
| `python` | `.py` `.pyi`                              |
| `go`     | `.go`                                     |
| `cs`     | `.cs`                                     |
| `sql`    | `.sql` `.plpgsql`                         |

```
$ code-moniker langs rs
lang: rs
kinds:
  namespace:  impl, module
  type:       enum, struct, trait, type
  callable:   fn, method
  value:      const, local, param, static
  annotation: comment
  ref:        annotates, calls, di_register, di_require, extends, implements,
              imports_module, imports_symbol, instantiates, method_call, reads,
              reexports, uses_type
visibilities: public, private, module
```

The kind union is `<lang>.allowed_kinds()` plus the cross-language ref kinds every extractor emits. Languages without access modelling report `visibilities: (none — ignored by this language)`.

## `shapes`

```
code-moniker shapes [--format text|json]
```

Every `kind` an extractor emits belongs to exactly one shape; refs share `ref` as marker.

| Shape        | Examples                                                                |
| ------------ | ----------------------------------------------------------------------- |
| `namespace`  | `module`, `namespace`, `schema`, `impl`                                 |
| `type`       | `class`, `struct`, `enum`, `interface`, `trait`, `table`, `view`, …     |
| `callable`   | `function`, `method`, `constructor`, `procedure`, `async_function`      |
| `value`      | `field`, `const`, `static`, `enum_constant`, `param`, `local`, …        |
| `annotation` | `comment`                                                               |
| `ref`        | `calls`, `imports_*`, `extends`, `uses_type`, … (every ref kind)        |

Shapes back the `--shape` filter on `extract` and the `shape` projection in `check`. Use them when a rule must match across languages — `--shape callable` picks up `fn`, `method`, `function`, `constructor` in one shot.

Source of truth: `code_moniker_core::core::shape::Shape`. Adding a kind requires it to map to one of the six variants; enforced by `every_allowed_kind_has_a_shape` in `crates/core/src/lang/mod.rs`.
