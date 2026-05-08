# 001 — resolution gaps observed in dogfood (esac, 2026-05-08)

Scope of this doc : **gaps that genuinely belong to the extension**.
Items that consumers can solve with the data the extension already
exposes (alias maps from `imports_symbol`, runtime-globals registry,
`this`/`super` receiver walk via `receiver_hint`) are out of scope and
handled caller-side.

## Reference dataset

Repo `esac` v2 dogfood : 176 modules, 16 211 refs.

| confidence | n | resolved (`?=`) | pct |
|---|---|---|---|
| external | 693 | 0 | 0% (expected) |
| imported | 512 | 458 | 89.5% |
| local | 3 220 | 2 767 | 85.9% |
| name_match | 11 786 | 2 354 | 20.0% |

V1 baseline (multi-pass resolver, 6 confidence labels):
`db/baseline/extraction_baseline_2026-05-07.json` côté ESAC.

## Gap 1 — method_call : receiver identifier name dropped

`receiver_hint` carries the **shape** of the receiver but not its
text. When the receiver is an `identifier` (`obj.foo()`), the literal
name `obj` is read by `receiver_hint` (`src/lang/ts/refs.rs:424-437`)
and immediately reduced to the constant string `b"identifier"` ; the
caller never sees `obj`.

```rust
"identifier" => b"identifier",
```

Without `obj`, downstream consumers cannot link the call to the
import that bound `obj`. Trivial alias-style resolution (consumer
holds an `imports_symbol` → local-name map) is impossible.

Sample (`src/api/explorer.ts`) :

```ts
import { z } from 'zod';
const schema = z.string().optional();
```

```
ref kind=imports_symbol  alias=z   target=…/external:zod/path:z
ref kind=method_call     receiver_hint=identifier   target=…/path:src/path:api/path:explorer/method:string()
ref kind=method_call     receiver_hint=member       target=…/path:src/path:api/path:explorer/method:optional()
```

The first method_call has receiver `z` (an import), but the receiver
text is dropped. Consumer cannot connect the call to `zod/z`.

Volume on esac : ~2 380 method_call orphans whose receiver is a
syntactic `identifier`. Requires only the receiver's textual
identifier, no type inference.

## Gap 2 — di_register Awilix detection narrow

`maybe_emit_di_register` (`src/lang/ts/refs.rs:104-145`) fires only
when the call expression has exactly one named argument that is a
bare `identifier` :

```rust
if reject || named != 1 { return; }
let Some(id) = the_id else { return };
```

Common Awilix patterns rejected :

```ts
container.register('repoStore', asFunction(makeRepoStore));        // named=2 ≠ 1
asFunction(({ pool, log }) => makeService(pool, log));             // arg kind = arrow_function
asFunction(makeService.bind(null, options));                       // arg kind = call_expression
container.register('thing', asClass(Thing).singleton());           // arg kind = call_expression
```

What fires :

```ts
asFunction(makeService);   // 1 named arg, identifier kind → emit
asClass(Repo);
asValue(Config);
```

Real esac sites missed (`src/core/runtime/container.ts`) :

```ts
container.register('readResource',  asFunction(makeReadResource).singleton());
container.register('writeResource', asFunction(makeWriteResource).singleton());
container.register('grepCorpus',    asFunction(makeGrepCorpus));
```

The `register(name, factoryCall)` shape is a chained call expression,
the `.singleton()` postfix is a chained call expression. Walking the
call chain inside the extension to recover the inner `identifier`
argument is straightforward AST traversal — does not require type
information.

Volume on esac :

| | n |
|---|---|
| v2 di_register | 59 |
| v1 di_register | 1 234 |
| ratio | 0.05 |
