# `code-moniker check` rule DSL

Reference for the rule grammar used by the `check` subcommand. The DSL is
declarative and lives entirely in TOML — each rule is one or more
`<lhs> <op> <rhs>` assertions combined with booleans and implications. A
rule fires (= a violation is emitted) when its assertion evaluates to
**false** on a given def or ref.

## Global File Exclusions

`[exclude].uris` keeps files outside a rule pack's review surface. In CLI
project checks, it is evaluated before extraction and rule evaluation:

```toml
[exclude]
uris = [
  "**/crates/core/tests/fixtures/**",
]
```

These are URI/path globs, not boolean rule expressions. They match against
the checked file's `file://...` URI plus normalized filesystem path forms.
Use `*` for one path segment, `?` for one character inside a segment, and
`**` for any number of path segments.

## Scopes

A rule is anchored to a **scope** by its TOML path:

| TOML path                           | Scope             | Iterates over    |
| ----------------------------------- | ----------------- | ---------------- |
| `[[<lang>.<kind>.where]]`           | Def of that kind  | `graph.defs()` filtered by lang + kind |
| `[[<lang>.shape.<shape>.where]]`    | Def of that shape | `graph.defs()` filtered by lang + canonical shape |
| `[[shape.<shape>.where]]`           | Def of that shape, **any lang** | `graph.defs()` filtered by canonical shape |
| `[[default.<kind>.where]]`          | Def of that kind, **any lang** | fallback when no `[<lang>.<kind>]` entry exists for the file's language |
| `[[refs.where]]`                    | Ref (poly-lang)   | `graph.refs()`   |
| `[[<lang>.refs.where]]`             | Ref (per lang)    | `graph.refs()` filtered by lang of source |

Top-level `[[refs.where]]` is the natural home for architectural invariants
that hold across languages ("the domain layer does not depend on
infrastructure"). The per-lang form exists only for conventions that are
genuinely language-specific (e.g. `kind = 'reexports'` only exists in TS/JS).

`[[default.<kind>.where]]` lets you state a rule once for a kind that
exists in several languages (`class`, `method`, `function`). It only
applies to a file when the file's language has **no** `[<lang>.<kind>]`
block for that kind — per-language rules win over the default.

Shape scopes are additive. A `[[shape.callable.where]]` rule and a
`[[rust.fn.where]]` rule can both evaluate on a Rust `fn`. If both define
the same `id`, the kind-specific rule wins for that kind.

## Grammar

Expressions are written in the `expr = "..."` string of a `where` rule.

### Synopsis

```text
<expr> ::=
    <predicate>
  | NOT <expr>
  | <expr> AND <expr>
  | <expr> OR <expr>
  | <expr> => <expr>
  | ( <expr> )

<predicate> ::=
    <string_projection> ( = | != ) <string_value_or_projection>
  | <string_projection> ( =~ | !~ ) <regex>
  | <number_expr> <number_operator> <number_expr>
  | mode( <domain_value_expr> ) ( = | != ) <value_or_projection>
  | <collection_expr> subset <collection_expr>
  | <moniker_projection> ( = | != ) <moniker_value_or_projection>
  | <moniker_projection> <moniker_relation_operator> <moniker_uri>
  | <moniker_projection> ~ '<path_pattern>'
  | any( <count_domain>, <expr> )
  | all( <count_domain>, <expr> )
  | none( <count_domain>, <expr> )
  | <segment_lookup> ( = | != ) <string_value_or_projection>
  | <segment_lookup> ( =~ | !~ ) <regex>
  | has_segment( '<segment_kind>', '<segment_name>' )
  | require( '<uri_pattern>' )
  | $<alias_name>

<string_projection> ::=
    name | kind | shape | visibility | text | confidence
  | parent.name | parent.kind | parent.shape
  | source.name | source.kind | source.shape | source.visibility
  | target.name | target.kind | target.shape | target.visibility
  | segment.name | segment.kind

<number_projection> ::=
    lines | depth

<number_expr> ::=
    <number>
  | <number_projection>
  | count( <count_domain> [, <expr> ] )
  | ( sum | max | min | avg | median | stddev | var | cv | gini )
      ( <item_domain>, <number_expr> )
  | percentile( <item_domain>, <number_expr>, <number> )
  | entropy( <domain_value_expr> )
  | size( <collection_expr> )
  | <metric_name>( self | each )

<domain_value_expr> ::=
    <item_domain>
  | <item_domain>, <value_expr>
  | <item_domain>.<projection_path>

<value_expr> ::=
    <string_projection>
  | <moniker_projection>
  | <number_expr>

<collection_expr> ::=
    <collection_projection>
  | <pair_collection_projection>
  | unique( <collection_expr> )
  | <collection_expr> ( intersect | union | diff ) <collection_expr>

<collection_projection> ::=
    <item_domain>[.<projection_path>]

<pair_collection_projection> ::=
    ( a | b ).<item_domain>[.<projection_path>]

<moniker_projection> ::=
    uri | moniker | source | source.parent | target | target.parent

<pair_projection> ::=
    ( a | b )[.<projection_path>]

<segment_lookup> ::=
    segment( '<segment_kind>' )
  | source.segment( '<segment_kind>' )
  | target.segment( '<segment_kind>' )

<count_domain> ::=
    <item_domain>
  | pairs( <item_domain> )

<item_domain> ::=
    <kind>
  | shape:<shape>
  | segment
  | out_refs
  | in_refs

<metric_name> ::=
    lcom4 | cbo | rfc | wmc | dit | noc | fan_in | fan_out

<string_value_or_projection>  ::= <string_value> | <string_projection> | <pair_projection>
<moniker_value_or_projection> ::= <moniker_uri> | <moniker_projection> | <pair_projection>
<value_or_projection>         ::= <string_value> | <string_projection> | <moniker_projection> | <pair_projection>

<number_operator>            ::= = | != | < | <= | > | >=
<moniker_relation_operator>  ::= @> | <@ | ?=

<path_pattern> ::=
    <path_step> [ / <path_step> ... ]

<uri_pattern> ::=
    <path_pattern_with_template_placeholders>

<path_step> ::=
    <kind>:<name>
  | <kind>:*
  | *:<name>
  | *
  | <kind>:/<regex>/
  | **
```

Operator precedence (loosest first): `=>`, `OR`, `AND`, `NOT`. Use parens
to override.

### Parameters

`<expr>`
: A boolean assertion evaluated for one def or one ref, depending on the
  rule scope. A rule emits a violation when `<expr>` is false.

`<predicate>`
: A single test. It can compare a projection, count a domain, evaluate a
  quantifier, match a moniker path, require a derived URI, check for a
  moniker segment, or expand an alias.

`<string_projection>`
: A projection that yields text. `name`, `visibility`, `text`,
  `parent.*`, and `segment.*` are def-scope projections. Unprefixed
  `kind` is the current def kind in def scope and the ref kind in ref
  scope. Unprefixed `shape` is the current def shape in def scope and the
  source def shape in ref scope. `confidence`, `source.*`, and `target.*`
  are ref-scope projections.

`<number_projection>`
: A numeric projection. `lines` is the 1-indexed line span of the current
  def body. `depth` is the number of segments in the current def moniker.

`<number_expr>`
: A numeric literal, projection, cardinality, aggregate, collection size,
  entropy, or named metric. Numeric expressions can appear on either side
  of numeric operators and inside numeric aggregators.

`<domain_value_expr>`
: A domain plus the value extracted from each item. `mode` returns the most
  frequent value and `entropy` returns the normalized entropy of the value
  distribution. The preferred spelling is `mode(out_refs, target.parent)`;
  legacy projection spelling such as `mode(out_refs.target.parent)` is also
  accepted.

`<collection_expr>`
: A multiset expression. A projection such as `method.name` evaluates the
  path for each item in the domain. `unique(...)` removes duplicates.
  `intersect`, `union`, and `diff` perform multiset algebra. Use
  `size(...)` to turn a collection into a number, or `subset` to compare
  two collections as multisets.

`<pair_projection>`
: A projection bound inside `pairs(...)` filters. `a` and `b` refer to the
  two items of the current pair. `a.name = b.name` compares their projected
  values; `a` or `b` without a suffix means the item's moniker when the
  item is a def or ref-backed value. Collection expressions inside a
  `pairs(...)` filter may also start from a pair binding. For example,
  `a.param.name` means the names of `param` children under pair item `a`.

`<moniker_projection>`
: A projection that yields a moniker URI. In def scope, `uri` and
  `moniker` are the current def. In ref scope, `uri`, `moniker`, and
  `source` are the ref source def; `source.parent` is the source def
  parent; `target` is the ref target; `target.parent` is the target
  moniker parent. Prefer `uri` in new rules and documentation; `moniker`
  remains accepted for compatibility with older rule packs.

`<segment_lookup>`
: Returns the first segment name with the requested kind, or `""` when the
  segment is absent. Use `segment(...)` in def scope and
  `source.segment(...)` / `target.segment(...)` in ref scope.

`<count_domain>`
: The collection inspected by `count`, `any`, `all`, or `none`.
  It accepts every `<item_domain>`, plus `pairs(D)`.

`<item_domain>`
: A collection of concrete local graph items.
  `<kind>` means direct child defs of that kind. `shape:<shape>` means
  direct child defs whose kind maps to the canonical shape. `segment` means
  moniker segments. `out_refs` and `in_refs` mean refs whose source or
  target is the current def. Aggregates, domain-value expressions, and
  collection projections use item domains; `pairs(D)` is only valid for
  `count`, `any`, `all`, and `none`.

`<metric_name>`
: One of the local named metrics. `self` binds to the rule's owner def.
  `each` binds to the item currently evaluated by an aggregate; outside an
  aggregate, `self` and `each` both point to the current def.

`<kind>`
: A def kind accepted by the current language, such as `class`, `method`,
  `function`, `field`, or `enum_constant`, plus internal kinds such as
  `module`, `comment`, `param`, and `local`. Use `code-moniker langs
  <lang>` to inspect the vocabulary.

`<string_value_or_projection>`
: A string literal or another string projection. For `=` and `!=`, a
  quoted right-hand value is always a string literal. An unquoted
  right-hand token that names a projection is evaluated as that projection.

`<regex>`
: A Rust regular expression used by `=~` and `!~`.

`<number_operator>`
: Numeric comparison between numeric expressions.

`<moniker_value_or_projection>`
: A full moniker URI or another moniker projection. `moniker = target`
  compares the current/source moniker to the ref target moniker.

`<moniker_relation_operator>`
: Moniker relationship comparison. `@>` means the left moniker is an
  ancestor of the right moniker, `<@` means descendant, and `?=` performs
  asymmetric `bind_match` for cross-file symbol resolution.

`<string_value>`
: A bare or quoted string. Quote values that contain whitespace or boolean
  boundary tokens such as `AND`, `OR`, or `=>`.

`<moniker_uri>`
: A full moniker URI parsed with the CLI scheme, usually
  `code+moniker://`.

`<path_pattern>`
: A slash-separated moniker path pattern. `**` matches zero or more
  segments; `*` matches one segment; `<kind>:/<regex>/` matches one segment
  of a fixed kind whose name matches the regex.

`has_segment( '<segment_kind>', '<segment_name>' )`
: Sugar for `moniker ~ '**/<segment_kind>:<segment_name>/**'`. In ref
  scope, `moniker` means the ref source def.

`$<alias_name>`
: Textual alias expansion from the effective alias table. The expanded
  expression is wrapped in parentheses before parsing.

## Semantics

### Operators

The op set is the moniker algebra plus comparison, regex, and collection
subset:

| Op    | Domain                | Meaning                                  |
| ----- | --------------------- | ---------------------------------------- |
| `=` `!=` | string / number / moniker | structural equality                   |
| `<` `<=` `>` `>=` | number       | numeric ordering                         |
| `=~` `!~` | string + regex    | regex match / no-match                   |
| `@>`  | moniker               | left is an ancestor of right             |
| `<@`  | moniker               | left is a descendant of right            |
| `?=`  | moniker               | asymmetric `bind_match` (per-lang arm)   |
| `~`   | moniker + path        | moniker matches the path pattern         |
| `subset` | collection         | left multiset is contained in right multiset |

### Booleans and implication

- `A AND B` — both true
- `A OR B` — at least one true
- `NOT A` — flip
- `A => B` — equivalent to `(NOT A) OR B`. Reads as "when A holds, B must
  hold". The most common form for architectural rules — without it,
  conjunctive rules end up flagging every def that doesn't match the
  premise.

### Quantifiers

`count(<count_domain>, <expr>?)` returns a cardinal and can be used anywhere a
number expression is accepted, including against another `count(...)`.
`any(<count_domain>, <expr>)`, `all(<count_domain>, <expr>)`, and
`none(<count_domain>, <expr>)` return booleans.

Domains:

| Domain      | Iterates over (relative to the current def/ref) |
| ----------- | ----------------------------------------------- |
| `<KIND>`    | direct children defs of that kind               |
| `shape:<S>` | direct children defs of shape `S`               |
| `pairs(D)`  | unordered pairs of distinct items from domain `D` |
| `segment`   | segments of the moniker (top-down)              |
| `out_refs`  | refs whose source is the current def            |
| `in_refs`   | refs whose target is the current def            |

The optional `<expr>` is evaluated with **the iterated item** as context,
so its projections refer to that item's attributes. For `segment`, only
`segment.kind` and `segment.name` are available. For `out_refs` / `in_refs`,
the full ref scope (`kind`, `source.*`, `target.*`) is in scope.

### Numeric analytics

Numeric aggregators evaluate a numeric expression once for each item of a
domain:

| Function | Meaning |
| -------- | ------- |
| `sum(D, E)` | sum of numeric values |
| `max(D, E)` / `min(D, E)` | maximum / minimum |
| `avg(D, E)` | arithmetic mean |
| `median(D, E)` | 50th percentile |
| `percentile(D, E, P)` | percentile `P`, from `0` to `100` |
| `stddev(D, E)` / `var(D, E)` | population standard deviation / variance |
| `cv(D, E)` | coefficient of variation, `stddev / abs(mean)` |
| `gini(D, E)` | Gini coefficient over non-negative numeric values |

Inside an aggregate, `each` binds to the iterated domain item and `self`
keeps pointing to the rule owner. This is useful for skew rules:

```toml
[[rust.struct.where]]
id   = "balanced-method-fanout"
expr = "count(method) >= 5 => cv(method, fan_out(each)) <= 0.6"
```

`entropy(D, E)` computes normalized entropy over any value expression,
not only numbers. `mode(D, E)` returns the most frequent value and can be
compared with `=` or `!=`.

```toml
[[rust.struct.where]]
id   = "shared-field-usage"
expr = "count(field) >= 3 => avg(field, entropy(in_refs, source)) >= 0.5"

[[rust.method.where]]
id   = "feature-envy"
expr = "count(out_refs) >= 5 => mode(out_refs, target.parent) = source.parent"
```

### Collections and multisets

A collection projection such as `method.name`, `out_refs.target.parent`,
or `field.in_refs.source.parent` returns a multiset. Duplicates are kept
unless `unique(...)` is used.

Available collection operations:

| Expression | Meaning |
| ---------- | ------- |
| `unique(M)` | remove duplicate values |
| `size(M)` | multiset cardinality as a number |
| `M1 intersect M2` | multiset intersection, minimum count per value |
| `M1 union M2` | multiset union, maximum count per value |
| `M1 diff M2` | multiset difference, saturating count subtraction |
| `M1 subset M2` | true when every value count in `M1` is present in `M2` |

The DSL deliberately uses ASCII keywords for collection algebra.

```toml
[[ts.class.where]]
id   = "unique-method-names"
expr = "size(unique(method.name)) = size(method.name)"

[[ts.class.where]]
id   = "fields-have-matching-methods"
expr = "field.name subset method.name"
```

Collection projections are local. `field.in_refs.source.parent` starts
from each direct field, follows that field's local incoming refs, and then
projects the parent of each ref source.

### Pair domains

`pairs(D)` is accepted in `count`, `any`, `all`, and `none`. It enumerates
unordered pairs of distinct items from `D`. Within the filter, `a` and `b`
bind the two pair items.

```toml
[[ts.class.where]]
id   = "no-duplicate-method-names"
expr = "count(pairs(method), a.name = b.name) = 0"

[[ts.class.where]]
id   = "method-pairs-owned-by-class"
expr = "all(pairs(method), a.parent = self AND b.parent = self)"

[[ts.class.where]]
id   = "no-data-clumps"
expr = "count(pairs(method), size(a.param.name intersect b.param.name) >= 3) = 0"
```

For a domain with zero or one item, `count(pairs(D))` is `0`; `all` over
the pair domain is vacuously true, and `any` is false.

### Named local metrics

Named metrics are numeric expressions with an explicit binding:

| Metric | Local formula |
| ------ | ------------- |
| `fan_in(X)` | refs whose target is `X` |
| `fan_out(X)` | refs whose source is `X` |
| `wmc(X)` | direct callable children, weight 1 per callable |
| `rfc(X)` | direct callables plus distinct targets called by those callables |
| `cbo(X)` | distinct external namespace/type buckets coupled through refs of `X` or descendants |
| `lcom4(X)` | connected components among direct callables, linked by method calls or shared direct field use |
| `dit(X)` | longest local inheritance chain through `extends`, `inherits`, `inheritance`, or `subclasses` refs |
| `noc(X)` | local children that inherit from `X` through those same ref kinds |

`X` is either `self` or `each`. Metrics are local to the file graph under
check. They do not use project-wide linkage or cross-file resolution.

```toml
[[rust.struct.where]]
id   = "low-lack-of-cohesion"
expr = "count(method) >= 4 => lcom4(self) <= 1"

[[java.class.where]]
id   = "lanza-marinescu-bounds"
expr = "cbo(self) <= 14 AND rfc(self) <= 50 AND wmc(self) <= 47"

[[java.class.where]]
id   = "inheritance-bounds"
expr = "dit(self) <= 5 AND noc(self) <= 10"
```

### Projections

In **def scope**, the bare attribute refers to the current def:

| Projection      | Source                                       |
| --------------- | -------------------------------------------- |
| `name`          | bare callable name of the last segment       |
| `kind`          | def kind                                     |
| `shape`         | def's canonical shape (see below)            |
| `visibility`    | def visibility                               |
| `lines`         | line count of the def's body                 |
| `depth`         | number of segments in the moniker            |
| `moniker`       | the moniker itself (operands: `=` `<@` `@>` `?=` `~`) |
| `parent.name`   | bare name of the moniker's penultimate segment |
| `parent.kind`   | kind of the moniker's penultimate segment    |
| `parent.shape`  | shape of the moniker's penultimate segment   |
| `segment(<K>)`  | name of the first segment of kind `K`, or `""` |

Files under common build layouts such as `src/main/...` and `src/test/...`
also carry a `srcset:main` or `srcset:test` segment near the start
of the moniker. This is useful for rules that apply only to production or
test sources, including languages such as Java where package canonicalization
otherwise replaces most path directories.

In **ref scope**, every projection is prefixed by `source.` or `target.`,
and an unprefixed `kind` refers to the ref kind (e.g. `calls`, `imports`,
`uses_type`, `implements`, `annotates`). Available projections on each
side: `name`, `kind`, `shape`, `visibility`, `moniker`, plus path matching
via `~` and `has_segment(...)` / `segment(...)`.

### Shape — the canonical kind grouping

`shape` collapses the 30+ per-language `kind` strings into five
language-agnostic buckets. It is the right projection for invariants that
hold *structurally* across languages, regardless of how each language
spells its keywords.

| Shape          | Kinds it covers                                                                 |
| -------------- | ------------------------------------------------------------------------------- |
| `namespace`    | `module`, `namespace`, `schema`, `impl`                                         |
| `type`         | `class`, `struct`, `interface`, `trait`, `enum`, `record`, `annotation_type`, `table`, `type`, `view`, `delegate` |
| `callable`     | `function`, `method`, `constructor`, `fn`, `func`, `procedure`, `async_function` |
| `value`        | `field`, `property`, `event`, `enum_constant`, `const`, `static`, `var`, `param`, `local` |
| `annotation`   | `comment`                                                                       |

```toml
# This block still iterates only over TypeScript classes, but the
# expression can speak in structural terms.
[[ts.class.where]]
id   = "class-name-pascalcase"
expr = "shape = 'type' => name =~ ^[A-Z][A-Za-z0-9]*$"

# Cross-language architectural rule expressed in shapes, not kinds.
[[refs.where]]
id   = "annotations-only-annotate"
expr = "source.shape = 'annotation' => kind = 'annotates'"
```

There is no top-level "all defs" scope. To cover several related kinds, use
a shape scope such as `[[shape.callable.where]]`; for one exact kind across
languages, use `[[default.<kind>.where]]` when a shared fallback rule is
acceptable.

The mapping table lives in `crates/core/src/core/shape.rs` and is the same one
exposed as the `shape` column of `graph_defs(code_graph)` in SQL — rules
written in shape terms transfer verbatim to ad-hoc queries against an
ingested `code_graph` corpus.

`target.visibility` requires that the ref's target is **resolved locally**
in the file under check. If the target is external (cross-file), that
projection is not applicable for the current ref and the atom is skipped
by the trivalent evaluator. Use `target.name` or `target ~ '<path>'` for
rules that should hold regardless of resolution status.

### Path patterns

`uri ~ '<path>'` (or `source ~ '...'`, `target ~ '...'`) matches a
moniker URI against a glob-like pattern composed of segments separated by
`/`. `moniker ~ '<path>'` is accepted as a compatibility alias for
`uri ~ '<path>'`:

| Step                 | Matches                                       |
| -------------------- | --------------------------------------------- |
| `module:domain`      | exact `(kind, name)` segment                  |
| `class:*`            | any name of that kind                         |
| `*:Foo`              | any kind with that name                       |
| `*`                  | exactly one segment, anything                 |
| `class:/^[A-Z][a-z]+Port$/` | regex on the name (kind fixed)         |
| `*:/^(api|web)$/`    | regex on the name, regardless of segment kind |
| `**`                 | zero or more segments (positional wildcard)   |

A pattern is anchored at the moniker's root unless it starts with `**/`.
`<@ <uri>` (subtree) is equivalent to `~ '<segments-of-uri>/**'`.
Use segment regexes to collapse repeated path-pattern ORs when only the
segment name varies, for example
`target ~ '**/*:/^(api|web|controller)$/**'` instead of three separate
`target ~ ...` arms.

### Derived requirements

`require('<uri-pattern>')` asserts that a derived URI exists. It is meant
for correlated-existence rules: the current symbol determines the address
of another required resource or symbol.

```toml
[[rust.enum.where]]
id = "command-variants-have-command-modules"
expr = """
uri ~ '**/module:args/enum:Command' => all(enum_constant,
  require("**/dir:crates/dir:cli/dir:src/dir:{name.snake}/module:mod")
)
"""
message = "CLI command variant `{name}` must have an owning module."
```

The pattern is rendered in the current item context. Supported template
placeholders are `{name}` and `{name.snake}`. In the example above, enum
constant `Stats` requires a `dir:stats/module:mod` URI.

`require(...)` is declarative from the rule author's point of view, but
the runner may resolve the expected URI lazily. In `check --file` mode, it
must not turn the invocation into a full project scan; it should resolve
only the concrete target files implied by the rendered requirement. A
repo-scoped `check` remains the completeness gate for edits that do not
touch the source symbol that drives the rule.

### Aliases

```toml
[aliases]
domain = "uri ~ '**/module:domain/**'"
infra  = "uri ~ '**/module:infrastructure/**'"
port   = "uri ~ '**/interface:/Port$/'"
```

A `$name` reference is substituted **textually** (wrapped in parens to
preserve precedence) before parsing. Aliases may reference other aliases
provided there is no cycle; unknown aliases and cycles are reported at
config load time.

The root `.code-moniker.toml` defines global aliases. A
`code-moniker.fragment.toml` can also define `[aliases]`; those names are
local to the fragment source file and are namespaced during merge. For a
fragment `fragment = "ui"`, local alias `panels` is stored as effective
alias `ui_panels`, and `$panels` in that fragment's rules or aliases is
rewritten to `$ui_panels`. Fragment aliases may reference global aliases.
They may also reference other local aliases. A fragment alias cannot
shadow an existing alias name, and its effective namespaced key cannot
collide with another alias. Aliases shared by several fragments should be
defined in the root `.code-moniker.toml`.

Because substitution is textual, an alias bundles **both** its
projection and its right-hand side. An alias written for def scope
(`uri ~ '...'`) can't be reused in ref scope by writing
`source $domain` — that would expand to `source (uri ~ '...')`
which is malformed. For ref-scope rules, write `source ~ '...'`
explicitly, or define separate aliases per scope (`src_domain`,
`tgt_domain`).

## Configuration topology

```
[aliases]                                  # optional, top-level
<name> = "<expr-or-fragment>"

[[<lang>.<kind>.where]]                    # def-scoped, lang-specific
id      = "..."
expr    = "..."
severity = "error"                         # optional; "error" (default) or "warn"
message = "..."                            # optional; templates are rendered
rationale = "..."                          # optional; rules-show metadata

[[shape.<shape>.where]]                    # def-scoped, cross-language shape
[[<lang>.shape.<shape>.where]]             # def-scoped, lang-specific shape
[[refs.where]]                             # ref-scoped, poly-lang
[[<lang>.refs.where]]                      # ref-scoped, lang-specific

[<lang>.<kind>]
require_doc_comment = "public"             # spatial rule, outside `where`
```

Merge: with `--default-rules on`, user TOML overrides the embedded preset
by rule id (replace in place) or appends new rules. `require_doc_comment`
overrides if set. Aliases from the user merge on top of embedded ones with
the same replace-by-name rule. With `--default-rules off`, the user TOML is
loaded as the complete config.

Fragments use the file name `code-moniker.fragment.toml` below the root
rules file directory:

```toml
fragment = "ui"                             # required
enabled = true                              # optional, default true

[aliases]                                  # optional, local to this fragment
panels = "uri ~ '**/dir:ui/dir:panels/**'"

[[refs.where]]
id      = "panels-ratatui-free"            # required in fragments
expr    = "$panels => NOT target ~ '**/external_pkg:ratatui/**'"
message = "`ui::panels` should stay renderer-free."
```

Fragments are merged after `.code-moniker.toml`. They do not support
overrides: a rule collision is an error. A fragment rule id is local and
gets the fragment id injected before it enters the effective config, so
the example rule id is `refs.ui.panels-ratatui-free`. Disabled fragments
remain visible in `rules show` but contribute no active rules or aliases.

`message` is rendered as the optional violation explanation. `severity`
accepts `"error"` or `"warn"` and defaults to `"error"` when omitted.
Warning rules are reported but do not make `check` exit `1` unless another
error-severity rule fails or a file read error occurs. `rationale` is
optional architectural context shown by `rules show`; it is not emitted as
a check violation explanation. Def rule messages can use `{name}`,
`{name.snake}`, `{kind}`, `{moniker}`, `{expr}`, `{value}`, `{expected}`
and the aliases `{pattern}`, `{lines}`, `{limit}`, `{count}`. Ref rule
messages can use `{kind}`, `{source.name}`, `{source.kind}`, `{source.shape}`,
`{source.moniker}`, `{target.name}`, `{target.kind}`, `{target.shape}`,
`{target.moniker}`, `{atom}`, `{actual}`, and `{expected}`.

## Recipes and suppression directives

Worked examples for layer boundaries, DDD contracts, adapters, test modules,
and doc comments live in the [recipes section of check](check.md#recipes).
Copyable, commented TOML samples live in
[check-samples](check-samples/README.md), with one file per supported
language.
Suppression directives live in [suppressions](check.md#suppressions). They
use this grammar; no new construct is introduced.

## Beyond direct refs

The DSL evaluates per def or per ref inside the local file graph. It can
derive local aggregates, multisets, pair checks, and named metrics from
the direct local defs/refs that were extracted for that file.

It still does not perform project-wide linkage, cross-file closure, call
graph transitive closure, cycle detection, or dataflow / taint
propagation. Those checks belong to SQL on an ingested `code_graph` corpus
or to a higher-level analysis pipeline.
