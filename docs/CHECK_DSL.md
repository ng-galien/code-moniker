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
attribute   := "name" | "kind" | "visibility" | "lines"
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
| `visibility`    | def visibility                               |
| `lines`         | line count of the def's body                 |
| `depth`         | number of segments in the moniker            |
| `moniker`       | the moniker itself (operands: `=` `<@` `@>` `?=` `~`) |
| `parent.name`   | bare name of the moniker's penultimate segment |
| `parent.kind`   | kind of the moniker's penultimate segment    |
| `segment(<K>)`  | name of the first segment of kind `K`, or `""` |

In **ref scope**, every projection is prefixed by `source.` or `target.`,
and an unprefixed `kind` refers to the ref kind (e.g. `calls`, `imports`,
`uses_type`, `implements`, `annotates`). Available projections on each
side: `name`, `kind`, `visibility`, `moniker`, plus path matching via `~`
and `has_segment(...)` / `segment(...)`.

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

A `$name` reference is substituted **textually** by its value before
parsing. Aliases may reference other aliases provided there is no cycle;
unknown aliases and cycles are reported at config load time, not at
evaluation.

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

A `.code-moniker.toml` that covers the main architecture patterns on a
hexagonal TS app with bounded contexts `billing` and `shipping`.

```toml
# ─── Aliases ──────────────────────────────────────────────────────────
[aliases]
domain   = "moniker ~ '**/module:domain/**'"
app      = "moniker ~ '**/module:application/**'"
infra    = "moniker ~ '**/module:infrastructure/**'"
adapter  = "moniker ~ '**/module:adapter/**'"
test     = "any(segment, segment.kind = 'module' AND segment.name =~ (^tests?$|_test$))"
std      = "moniker ~ '**/module:std/**' OR moniker ~ '**/module:core/**'"
port     = "moniker ~ '**/interface:/Port$/'"
contract = "moniker ~ '**/module:contract/**'"

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
[[refs.where]]
id   = "domain-depends-on-nothing-but-itself-or-std"
expr = "source $domain => target $domain OR target $std"

[[refs.where]]
id   = "application-only-inward"
expr = "source $app => target $app OR target $domain OR target $std"

[[refs.where]]
id   = "domain-imports-no-framework"
expr = """
  source $domain AND kind = 'imports'
  => NOT target.name =~ ^(express|nestjs|typeorm|prisma)$
"""

[[refs.where]]
id   = "infra-implements-application-ports-only"
expr = """
  source $infra AND kind = 'implements'
  => target $app AND target.name =~ Port$
"""

# ─── Bounded contexts ─────────────────────────────────────────────────
[[refs.where]]
id   = "billing-touches-shipping-only-via-contract"
expr = """
  source.segment('module') = 'billing'
  AND target.segment('module') = 'shipping'
  => target $contract
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
                   AND target $app
                   AND target.name =~ Port$)
"""

# ─── Couplage ─────────────────────────────────────────────────────────
[[ts.class.where]]
id   = "low-fan-out"
expr = """
  kind = 'class'
  => count(out_refs, kind = 'uses_type' AND NOT target $std) <= 7
"""

# ─── Tests ────────────────────────────────────────────────────────────
[[ts.class.where]]
id   = "no-class-in-config-or-fixture-only-module"
expr = "$test => NOT name =~ ^(Stub|Mock|Fake|Builder)$"

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

## Limits

Three classes of invariants are **out of scope** of this DSL and live in
SQL on `code_graph`:

- Transitive closure ("X indirectly calls Y").
- Cycle detection.
- Dataflow / taint propagation.

Anything that requires walking the graph beyond direct refs of the current
def is intentionally not expressible here — the linter is per-file and
runs in a hook on each edit. Cross-file invariants belong to a separate
SQL query that runs in CI or against an ingested code_graph corpus.
