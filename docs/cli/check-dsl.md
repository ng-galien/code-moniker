# `code-moniker check` rule DSL

Reference for the rule grammar used by the `check` subcommand. The DSL is
declarative, side-effect-free, and lives entirely in TOML — each rule is
one or more `<lhs> <op> <rhs>` assertions combined with booleans and
implications. A rule fires (= a violation is emitted) when its assertion
evaluates to **false** on a given def or ref.

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
  | <moniker_projection> ( = | != ) <moniker_value_or_projection>
  | <moniker_projection> <moniker_relation_operator> <moniker_uri>
  | <moniker_projection> ~ '<path_pattern>'
  | any( <domain>, <expr> )
  | all( <domain>, <expr> )
  | none( <domain>, <expr> )
  | <segment_lookup> ( = | != ) <string_value_or_projection>
  | <segment_lookup> ( =~ | !~ ) <regex>
  | has_segment( '<segment_kind>', '<segment_name>' )
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
  | count( <domain> [, <expr> ] )

<moniker_projection> ::=
    moniker | source | source.parent | target | target.parent

<segment_lookup> ::=
    segment( '<segment_kind>' )
  | source.segment( '<segment_kind>' )
  | target.segment( '<segment_kind>' )

<domain> ::=
    <kind>
  | shape:<shape>
  | segment
  | out_refs
  | in_refs

<string_value_or_projection>  ::= <string_value> | <string_projection>
<moniker_value_or_projection> ::= <moniker_uri> | <moniker_projection>

<number_operator>            ::= = | != | < | <= | > | >=
<moniker_relation_operator>  ::= @> | <@ | ?=

<path_pattern> ::=
    <path_step> [ / <path_step> ... ]

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
  quantifier, match a moniker path, check for a moniker segment, or expand
  an alias.

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

`<moniker_projection>`
: A projection that yields a moniker. In def scope, `moniker` is the
  current def. In ref scope, `moniker` and `source` are the ref source def;
  `source.parent` is the source def parent; `target` is the ref target;
  `target.parent` is the target moniker parent.

`<segment_lookup>`
: Returns the first segment name with the requested kind, or `""` when the
  segment is absent. Use `segment(...)` in def scope and
  `source.segment(...)` / `target.segment(...)` in ref scope.

`<domain>`
: The collection inspected by `count`, `any`, `all`, or `none`.
  `<kind>` means direct child defs of that kind. `shape:<shape>` means
  direct child defs whose kind maps to the canonical shape. `segment`
  means moniker segments. `out_refs` and `in_refs` mean refs whose source
  or target is the current def.

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
: Numeric comparison against an unsigned integer literal.

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
: Textual alias expansion from the top-level `[aliases]` table. The
  expanded expression is wrapped in parentheses before parsing.

## Semantics

### Operators

The op set is the moniker algebra plus comparison and regex:

| Op    | Domain                | Meaning                                  |
| ----- | --------------------- | ---------------------------------------- |
| `=` `!=` | string / number / moniker | structural equality                   |
| `<` `<=` `>` `>=` | number       | numeric ordering                         |
| `=~` `!~` | string + regex    | regex match / no-match                   |
| `@>`  | moniker               | left is an ancestor of right             |
| `<@`  | moniker               | left is a descendant of right            |
| `?=`  | moniker               | asymmetric `bind_match` (per-lang arm)   |
| `~`   | moniker + path        | moniker matches the path pattern         |

### Booleans and implication

- `A AND B` — both true
- `A OR B` — at least one true
- `NOT A` — flip
- `A => B` — equivalent to `(NOT A) OR B`. Reads as "when A holds, B must
  hold". The most common form for architectural rules — without it,
  conjunctive rules end up flagging every def that doesn't match the
  premise.

### Quantifiers

`count(<domain>, <expr>?)` returns a cardinal and can be used anywhere a
number expression is accepted, including against another `count(...)`.
`any(<domain>, <expr>)`, `all(<domain>, <expr>)`, `none(<domain>, <expr>)`
return booleans.

Domains:

| Domain      | Iterates over (relative to the current def/ref) |
| ----------- | ----------------------------------------------- |
| `<KIND>`    | direct children defs of that kind               |
| `shape:<S>` | direct children defs of shape `S`               |
| `segment`   | segments of the moniker (top-down)              |
| `out_refs`  | refs whose source is the current def            |
| `in_refs`   | refs whose target is the current def            |

The optional `<expr>` is evaluated with **the iterated item** as context,
so its projections refer to that item's attributes. For `segment`, only
`segment.kind` and `segment.name` are available. For `out_refs` / `in_refs`,
the full ref scope (`kind`, `source.*`, `target.*`) is in scope.

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
in the file under check; if the target is external (cross-file), the rule
errors out at evaluation rather than skipping silently. Use `target.name`
or `target ~ '<path>'` for rules that should hold regardless of resolution
status.

### Path patterns

`moniker ~ '<path>'` (or `source ~ '...'`, `target ~ '...'`) matches a
moniker against a glob-like pattern composed of segments separated by `/`:

| Step                 | Matches                                       |
| -------------------- | --------------------------------------------- |
| `module:domain`      | exact `(kind, name)` segment                  |
| `class:*`            | any name of that kind                         |
| `*:Foo`              | any kind with that name                       |
| `*`                  | exactly one segment, anything                 |
| `class:/^[A-Z][a-z]+Port$/` | regex on the name (kind fixed)         |
| `**`                 | zero or more segments (positional wildcard)   |

A pattern is anchored at the moniker's root unless it starts with `**/`.
`<@ <uri>` (subtree) is equivalent to `~ '<segments-of-uri>/**'`.

### Aliases

```toml
[aliases]
domain = "moniker ~ '**/module:domain/**'"
infra  = "moniker ~ '**/module:infrastructure/**'"
port   = "moniker ~ '**/interface:/Port$/'"
```

A `$name` reference is substituted **textually** (wrapped in parens to
preserve precedence) before parsing. Aliases may reference other
aliases provided there is no cycle; unknown aliases and cycles are
reported at config load time.

Because substitution is textual, an alias bundles **both** its
projection and its right-hand side. An alias written for def scope
(`moniker ~ '...'`) can't be reused in ref scope by writing
`source $domain` — that would expand to `source (moniker ~ '...')`
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
message = "..."                            # optional; templates are rendered

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

`message` is rendered as the optional violation explanation. Def rules can
use `{name}`, `{kind}`, `{moniker}`, `{expr}`, `{value}`, `{expected}` and
the aliases `{pattern}`, `{lines}`, `{limit}`, `{count}`. Ref rules can use
`{kind}`, `{source.name}`, `{source.kind}`, `{source.shape}`,
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

The DSL evaluates per def or per ref, looking at direct refs of the
current node. Transitive closure (`X indirectly calls Y`), cycle
detection, and dataflow / taint propagation are expressed as SQL on
`code_graph`, not as rules. Cross-file invariants belong to a separate
SQL query that runs in CI or against an ingested code_graph corpus.
