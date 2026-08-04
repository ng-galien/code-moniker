# One-to-one method/helper graph query

## Observed simplification signal

A hollow method/helper split appears as a repeated local graph motif:

```text
type T --contains--> method M --calls--> free function F --uses_type--> T
                                      ^
                                      one incoming call
```

For a type `T`, define:

```text
satellite_score(T) = count of (M, F) where:
- M is a method directly owned by T;
- M spans at most 13 lines;
- M has exactly one `calls` edge to the local free function F;
- F has exactly one incoming `calls` edge;
- F has a `uses_type` edge back to T.
```

The score does not classify every thin delegation as redundant. A shared helper has
call fan-in greater than one, and a facade over a different subsystem does not point
back to its owner type.

## Code Moniker experiment

On 2026-08-04, the local check DSL could express the uncorrelated precursor:

```toml
[[rust.shape.type.where]]
id = "experiment-delegation-cluster"
severity = "warn"
expr = "NOT (count(method, lines <= 13 AND count(out_refs, kind = 'calls' AND target.kind = 'fn') = 1) >= 5)"
```

That rule selected 5 of 1,380 Rust types, but still mixed one-to-one satellites
with shared helpers. A direct traversal over Code Moniker's extracted defs and refs
then evaluated the correlated score over all 780 scanned files:

| type | score |
|---|---:|
| `LinkageStore` | 5 |
| `WorkspaceLiveRefreshPlan` | 4 |
| `CompiledRules` | 2 |
| `TsSdkProfile` | 2 |

Every other type scored at most 1. `LocalCodeIndex` and `SymbolInventoryFacets`,
both selected by the precursor rule, scored 0 because their targets are shared or
do not consume the owner type.

## DSL evolution

The graph already contained every required fact. No path language was needed:
the existing nested quantifiers already preserve the outer reference as `current`.
The minimal missing operation was the ability to traverse the current reference's
local target through two new domains:

- `target.out_refs`;
- `target.in_refs`.

That makes the correlated score directly expressible:

```text
count(method,
  lines <= 13
  AND count(out_refs,
    kind = 'calls' AND target.kind = 'fn' AND target.visibility = 'private'
  ) = 1
  AND any(out_refs,
    kind = 'calls'
    AND target.kind = 'fn'
    AND target.visibility = 'private'
    AND count(target.in_refs, kind = 'calls') = 1
    AND any(target.out_refs,
      kind = 'uses_type'
      AND target = current.source.parent
    )
  )
)
```

Inside `target.out_refs`, `current` is the call reference. Its
`source.parent` is therefore the method owner `T`. A non-local target exposes no
target refs, so this operation cannot accidentally turn into a workspace-wide
lookup.

## Dogfood result after implementation

The project warning rule allows one isolated satellite and reports owners with at
least two. On the 2026-08-04 dogfood index (745 scanned files, 1,372 evaluated
Rust types), it reports exactly four owners:

| type | score |
|---|---:|
| `LinkageStore` | 5 |
| `WorkspaceLiveRefreshPlan` | 4 |
| `CompiledRules` | 2 |
| `TsSdkProfile` | 2 |

The result reproduces the direct graph experiment without introducing a general
graph-query abstraction. The DSL gained two local ref domains; correlation,
fan-in and owner identity remain compositions of existing primitives.

This should remain a warning/ranking metric. A high score is a precise review
candidate for subtraction, not proof that the split is invalid.
