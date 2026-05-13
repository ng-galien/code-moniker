# `code-moniker check` rule DSL

Reference for the rule grammar used by the `check` subcommand. The DSL is
declarative, side-effect-free, and lives entirely in TOML — each rule is
one or more `<lhs> <op> <rhs>` assertions combined with booleans and
implications. A rule fires (= a violation is emitted) when its assertion
evaluates to **false** on a given def or ref.

## Scopes

A rule is anchored to one of three **scopes** by its TOML path:

| TOML path                           | Scope             | Iterates over    |
| ----------------------------------- | ----------------- | ---------------- |
| `[[<lang>.<kind>.where]]`           | Def of that kind  | `graph.defs()` filtered by lang + kind |
| `[[refs.where]]`                    | Ref (poly-lang)   | `graph.refs()`   |
| `[[<lang>.refs.where]]`             | Ref (per lang)    | `graph.refs()` filtered by lang of source |

Top-level `[[refs.where]]` is the natural home for architectural invariants
that hold across languages ("the domain layer does not depend on
infrastructure"). The per-lang form exists only for conventions that are
genuinely language-specific (e.g. `kind = 'reexports'` only exists in TS/JS).

## Grammar

```text
expr        := implication
implication := disjunction ( "=>" disjunction )?
disjunction := conjunction ( "OR" conjunction )*
conjunction := negation ( "AND" negation )*
negation    := "NOT" negation | primary
primary     := atom | "(" expr ")"

atom        := projection op rhs
             | quantifier "(" domain ( "," expr )? ")"
             | path_match
             | "$" IDENT                         # alias reference

quantifier  := "count" | "any" | "all" | "none"
domain      := KIND_IDENT | "segment" | "out_refs" | "in_refs"

projection  := scope_prefix? attribute
scope_prefix:= ( "source" | "target" | "parent" | "segment" ) "."
attribute   := "name" | "kind" | "shape" | "visibility" | "lines"
             | "depth" | "moniker"

path_match  := projection? "~" PATH_STRING
PATH_STRING := "'" path "'"
path        := step ( "/" step )*
step        := <kind>":"<name>                   # literal
             | <kind>":*"                        # any name of that kind
             | "*:"<name>                        # any kind, that name
             | "*"                               # any single segment
             | <kind>":/"<regex>"/"              # regex on name
             | "**"                              # 0+ segments

op          := "=" | "!=" | "<" | "<=" | ">" | ">=" | "=~" | "!~"
             | "@>" | "<@" | "?="
rhs         := NUMBER | STRING | MONIKER_URI | PROJECTION
```

Operator precedence (loosest first): `=>`, `OR`, `AND`, `NOT`. Use parens
to override.

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

`count(<domain>, <expr>?) <op> <number>` returns a cardinal.
`any(<domain>, <expr>)`, `all(<domain>, <expr>)`, `none(<domain>, <expr>)`
return booleans.

Domains:

| Domain      | Iterates over (relative to the current def/ref) |
| ----------- | ----------------------------------------------- |
| `<KIND>`    | direct children defs of that kind               |
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
# Single rule that fires on any type-shape def across all 7 languages.
[[ts.class.where]]
expr = "shape = 'type' => name =~ ^[A-Z][A-Za-z0-9]*$"

# Cross-language architectural rule expressed in shapes, not kinds.
[[refs.where]]
id   = "annotations-only-annotate"
expr = "source.shape = 'annotation' => kind = 'annotates'"
```

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
message = "..."                            # optional, custom template

[[refs.where]]                             # ref-scoped, poly-lang
[[<lang>.refs.where]]                      # ref-scoped, lang-specific

[<lang>.<kind>]
require_doc_comment = "public"             # spatial rule, outside `where`
```

Merge: user TOML overrides the embedded preset by rule id (replace in
place) or appends new rules. `require_doc_comment` overrides if set.
Aliases from the user merge on top of embedded ones with the same
replace-by-name rule.

## Worked example — Hex / DDD / Clean Code

A `.code-moniker.toml` for a TypeScript app organised as
`src/domain/`, `src/application/`, `src/infrastructure/`,
`src/billing/`, `src/shipping/`. TS path segments are encoded as
`dir:<seg>`, so an alias matches against `**/dir:domain/**`.

```toml
[aliases]
domain   = "moniker ~ '**/dir:domain/**'"
app      = "moniker ~ '**/dir:application/**'"
infra    = "moniker ~ '**/dir:infrastructure/**'"
adapter  = "moniker ~ '**/dir:adapter/**'"
contract = "moniker ~ '**/dir:contract/**'"

# ─── Clean Code (def-scoped, per-lang) ────────────────────────────────
[[ts.class.where]]
id   = "no-god-class"
expr = "count(method) <= 20 AND count(field) <= 7 AND all(method, lines <= 60)"

[[ts.method.where]]
id   = "name-is-a-verb"
expr = "NOT name =~ ^(do|handle|process|manage)[A-Z]"

[[ts.constructor.where]]
id   = "small-ctor"
expr = "count(param) <= 4"

[[ts.method.where]]
id   = "method-not-named-after-parent"
expr = "name != parent.name"

# ─── DDD building blocks ──────────────────────────────────────────────
[[ts.class.where]]
id   = "entity-has-id"
expr = "name =~ Entity$ => any(field, name = 'id')"

[[ts.class.where]]
id   = "value-object-immutable"
expr = """
  (name =~ VO$ OR name =~ Value$)
  => all(field, visibility = 'private')
     AND none(method, name =~ ^set)
"""

[[ts.interface.where]]
id   = "repository-lives-in-domain"
expr = "name =~ Repository$ => $domain"

[[ts.class.where]]
id   = "use-case-shape"
expr = """
  name =~ UseCase$
  => count(method) = 1
     AND any(method, name = 'execute')
"""

[[ts.class.where]]
id   = "port-is-never-a-class"
expr = "NOT name =~ Port$"

# ─── Hex / Layering (poly-lang refs) ──────────────────────────────────
# Aliases substitute textually — `source $domain` would expand to
# `source (moniker ~ '...')` which is malformed. In refs scope, use the
# explicit `source ~` / `target ~` form instead.
[[refs.where]]
id   = "domain-depends-on-nothing-but-itself"
expr = """
  source ~ '**/dir:domain/**'
  => target ~ '**/dir:domain/**'
"""

[[refs.where]]
id   = "application-only-inward"
expr = """
  source ~ '**/dir:application/**'
  => target ~ '**/dir:application/**'
     OR target ~ '**/dir:domain/**'
"""

[[refs.where]]
id   = "domain-imports-no-framework"
expr = """
  source ~ '**/dir:domain/**' AND kind = 'imports_symbol'
  => NOT (target ~ '**/external_pkg:express/**'
          OR target ~ '**/external_pkg:nestjs/**'
          OR target ~ '**/external_pkg:typeorm/**'
          OR target ~ '**/external_pkg:prisma/**')
"""

[[refs.where]]
id   = "infra-implements-application-ports-only"
expr = """
  source ~ '**/dir:infrastructure/**' AND kind = 'implements'
  => target ~ '**/dir:application/**' AND target.name =~ Port$
"""

# ─── Bounded contexts ─────────────────────────────────────────────────
[[refs.where]]
id   = "billing-touches-shipping-only-via-contract"
expr = """
  source ~ '**/dir:billing/**' AND target ~ '**/dir:shipping/**'
  => target ~ '**/dir:contract/**'
"""

# ─── Adapters & controllers ───────────────────────────────────────────
[[ts.class.where]]
id   = "thin-controller"
expr = """
  name =~ Controller$
  => count(method) <= 8
     AND count(class) = 0
     AND all(method, lines <= 30)
"""

[[ts.class.where]]
id   = "adapter-implements-a-port"
expr = """
  name =~ Adapter$
  => any(out_refs, kind = 'implements'
                   AND target ~ '**/dir:application/**'
                   AND target.name =~ Port$)
"""

# ─── Coupling ─────────────────────────────────────────────────────────
[[ts.class.where]]
id   = "low-fan-out"
expr = """
  kind = 'class'
  => count(out_refs, kind = 'uses_type'
                     AND NOT target ~ '**/external_pkg:/**') <= 7
"""

# ─── Tests ────────────────────────────────────────────────────────────
[[ts.class.where]]
id   = "fixtures-only-in-test-modules"
expr = """
  name =~ ^(Stub|Mock|Fake|Builder)$
  => any(segment, segment.kind = 'dir'
                  AND segment.name =~ (^tests?$|_test$))
"""

# ─── Doc comment (spatial, outside the DSL) ───────────────────────────
[ts.class]
require_doc_comment = "public"
```

## Suppression directives

```ts
// code-moniker: ignore                              // suppress every rule on the next def
// code-moniker: ignore[name-pascalcase]             // only that rule id (suffix match)
// code-moniker: ignore-file                         // whole file
// code-moniker: ignore-file[max-lines]              // whole file, single rule
```

Rule ids follow the TOML path: `<lang>.<kind>.<id>` for def rules,
`refs.<id>` for top-level ref rules, `<lang>.refs.<id>` for lang-specific
ones. Suffix match: `name-pascalcase` matches every
`<lang>.<kind>.name-pascalcase`.

## Beyond direct refs

The DSL evaluates per def or per ref, looking at direct refs of the
current node. Transitive closure (`X indirectly calls Y`), cycle
detection, and dataflow / taint propagation are expressed as SQL on
`code_graph`, not as rules. Cross-file invariants belong to a separate
SQL query that runs in CI or against an ingested code_graph corpus.
