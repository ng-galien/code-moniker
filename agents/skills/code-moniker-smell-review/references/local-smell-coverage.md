# Local Smell Coverage Reference

## DSL Capabilities

The current check DSL can express first-order local graph rules over one
file's extracted code graph:

- Boolean logic and implication: `AND`, `OR`, `NOT`, `=>`, and `disjoint`
  for symmetric mutual exclusion (`A disjoint B` is `NOT (A AND B)`, so one
  rule replaces a mirrored pair of `=>` rules).
- Domains: direct child kinds, `shape:<shape>`, `segment`, `out_refs`,
  `in_refs`, ref-relative `source.*_refs` / `target.*_refs`, and simple
  `pairs(D)` filters.
- Quantifiers and counts: `count`, `any`, `all`, `none`.
- Descriptive statistics: `sum`, `max`, `min`, `avg`, `median`,
  `percentile`, `stddev`, `var`, `cv`, `gini`, `entropy`, and `mode`.
- Multiset algebra: `unique`, `size`, `intersect`, `union`, `diff`,
  `subset`.
- Local OO metrics: `lcom4`, `cbo`, `rfc`, `wmc`, `dit`, `noc`,
  `fan_in`, and `fan_out`.

The indexed `workspace.group` root additionally supports boolean combinations
of `count(member)` and `sum`, `max`, `min`, `avg`, `median`, `percentile`,
`stddev`, `var`, `cv`, or `gini` over `(member, lines)`. Groups are selected
and maintained as Roaring bitmaps; line spans come from inventory records.
Missing spans fail closed with coverage, and implications should guard small
samples. Boolean order is semantically neutral: a decisive known `AND`/`OR`
operand wins, while unresolved compositions remain fail-closed.

## Executable Local Smell Families

Use warning rules for:

- Long Method / Long Function: `lines <= N` on `shape.callable`.
- Long Parameter List: `count(param) <= N` on `shape.callable`.
- Large Class / Large Type: caps on `count(shape:callable)` and
  `count(shape:value)` on `shape.type`.
- Lazy Class: minimum direct callable or value count on `shape.type`.
- Feature Envy approximation: sufficient outgoing refs imply the modal
  target parent is the source parent.
- God Class / Brain Class local approximation: guarded `wmc`, `cbo`,
  `lcom4` bounds.
- Inheritance abuse: `dit` and `noc` bounds.
- Distribution disharmony: `cv` over method lines and `gini` over method
  fan-out.
- Data Clumps: `count(pairs(method), size(a.param.name intersect
  b.param.name) >= 3) = 0`.
- Caller concentration: normalized `entropy(in_refs, source.parent)`.
- One-to-one method/helper satellites: correlate a method call with
  `target.in_refs`, `target.out_refs`, and `current.source.parent`.
- Duplicate child names: `size(unique(shape:callable.name))`.
- Comment smell: comment length or TODO/HACK patterns on
  `shape.annotation`.

## Current Near Misses

Document these as evolutions unless the codebase already added support:

- General numeric arithmetic (`+`, `-`, `*`, `/`) for ratios such as
  Middle Man or Data Class accessor fractions.
- `fraction(D, F)` as sugar for `count(D, F) / size(D)`.
- `param.type` and `field.type` projections for Primitive Obsession and
  typed Data Clumps.
- `cyclo` and `max_nesting` numeric projections for Brain Method.
- Corpus-wide baselines such as percentile/z-score across all project
  symbols for arbitrary projections. Member-line percentiles inside a
  `workspace.group` are supported.

## Out Of Scope For CLI Local Rules

Escalate these to SQL/PG corpus analysis, extractor work, or a separate
tool:

- Divergent Change, Shotgun Surgery, and Parallel Inheritance Hierarchies:
  need change history or cross-file co-change.
- Duplicate Code: needs clone detection.
- Message Chains: needs transitive call/property chains, not direct refs.
- Temporary Field: needs data-flow/reaching-defs.
- Switch Statements: needs AST control-flow shape unless text heuristics
  are explicitly accepted.
