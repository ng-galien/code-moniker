# Code smell review with the local check DSL

This page records what the `code-moniker check` DSL can do today for
Fowler-style code smells and Lanza/Marinescu-style metric heuristics. It
is intentionally scoped to the local CLI graph: one extracted file, direct
defs, local refs, local metrics, and local distribution statistics.

The review stance is conservative: smell rules are heuristic review
signals. Encode them as `severity = "warn"` unless a project deliberately
wants to enforce one as an error.

## DSL model

The rule DSL is a first-order algebra over a local code graph with three
useful layers.

| Layer | Operators | Review use |
| ----- | --------- | ---------- |
| Logic | `AND`, `OR`, `NOT`, `=>`, `count`, `any`, `all`, `none`, `pairs(D)` | express guarded smell predicates without flagging symbols outside the premise |
| Descriptive statistics | `sum`, `max`, `min`, `avg`, `median`, `percentile`, `stddev`, `var`, `cv`, `gini`, `entropy`, `mode` | detect local distribution imbalance and caller concentration |
| Multiset algebra | `unique`, `intersect`, `union`, `diff`, `size`, `subset`, chained projections | compare local child/ref projections and uniqueness invariants |

The named OO metrics are local-only: `lcom4`, `cbo`, `rfc`, `wmc`, `dit`,
`noc`, `fan_in`, and `fan_out`. They are useful for local approximations
of Lanza/Marinescu checks, but they do not use cross-file linkage or a
project-wide baseline.

## Executable smell coverage

These smells can be encoded as local warning rules today.

| Smell family | Executable DSL shape | Notes |
| ------------ | -------------------- | ----- |
| Long Method | `lines <= N` on `shape.callable` | Fowler smell, threshold is project policy |
| Long Parameter List | `count(param) <= N` | direct parameter defs under callables |
| Large Class / Large Type | `count(shape:callable) <= N` and `count(shape:value) <= M` | no arithmetic needed |
| Lazy Class | `count(shape:callable) >= 2 OR count(shape:value) >= 2` | intentionally broad and noisy |
| Feature Envy | `count(out_refs) >= 5 => mode(out_refs, target.parent) = source.parent` | local approximation of external access concentration |
| God Class / Brain Class approximation | guarded `wmc`, `cbo`, `lcom4` bounds | local Lanza/Marinescu-style thresholds |
| Inheritance abuse | `dit(self) <= 5 AND noc(self) <= 10` | local inheritance only |
| Data Clumps | `count(pairs(method), size(a.param.name intersect b.param.name) >= 3) = 0` | pair-bound collection projections compare repeated parameter groups |
| Distribution disharmony | `cv(shape:callable, lines)` and `gini(shape:callable, fan_out(each))` | captures uneven method sizes or a hidden coupling hub |
| Caller concentration | `entropy(in_refs, source.parent) >= 0.5` after a volume guard | low entropy means one owner dominates local use |
| Duplicate child names | `size(unique(shape:callable.name)) = size(shape:callable.name)` | local naming invariant |
| Comments smell | `shape.annotation` checks over `lines` or `text` | project-specific thresholds |

Use the sample warning pack at
[code-smells-local.toml](check-samples/code-smells-local.toml) as the
copyable starting point.

## Important current gaps

Some useful expressions from the design space are not executable yet.
Document them as evolutions instead of writing invalid TOML.

| Gap | Smells unlocked | Why it is missing |
| --- | --------------- | ----------------- |
| Numeric arithmetic in `number_expr` | Data Class ratios, Middle Man ratios, LAA/BUR/BOvR-style metrics | the grammar accepts numeric expressions but not `+`, `-`, `*`, `/` operators |
| `fraction(D, F)` sugar | Data Class and Middle Man without general arithmetic | not implemented |
| `param.type` / `field.type` projections | Primitive Obsession and typed Data Clumps | type data is not exposed through check projections |
| `cyclo` and `max_nesting` | Brain Method / Brain Class refinements | requires extractor-side AST metrics per language |
| Corpus-wide baselines | Lanza/Marinescu Few/Many/High/Very High thresholds | the CLI model is per-file; project baselines fit better in SQL over ingested graphs |

## Out of local CLI scope

The following smells need data the local DSL intentionally does not own:

- Divergent Change, Shotgun Surgery, and Parallel Inheritance Hierarchies:
  change history or co-change analysis.
- Duplicate Code: clone detection.
- Message Chains: transitive call/property-chain analysis.
- Temporary Field: reaching-defs or data-flow analysis.
- Switch Statements: AST control-flow shape, unless a project accepts a
  brittle `text =~ ...` heuristic.

Use the PostgreSQL `code_graph` layer or a dedicated analyzer for those
checks.

## Review workflow

Validate the warning pack before a broad run:

```sh
code-moniker rules show . \
  --rules docs/cli/check-samples/code-smells-local.toml \
  --default-rules off
```

Run the review:

```sh
code-moniker check . \
  --rules docs/cli/check-samples/code-smells-local.toml \
  --default-rules off \
  --report \
  --max-violations 50
```

Interpret failures as review candidates. A warning says "inspect this
symbol", not "this symbol is wrong".
