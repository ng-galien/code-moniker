# Evolution: G10 vertical separation rule support

## Context

Clean Code G10 says variables and private functions should be declared near
where they are used. In code-moniker terms, this is attractive because it
turns source organization into a review signal: public surface first, then
private helpers close to the first caller, and local values close to first
read.

The current check DSL cannot express this honestly. `DefRecord` and
`RefRecord` already carry byte positions, and check diagnostics already
convert byte spans to line ranges, but rule expressions expose only `lines`
and `depth`. There is no projection for declaration start/end, reference
start/end, or line distance.

## Desired rule family

There are two useful levels.

The small level is a position-distance predicate. Start as warning-level
smell rules, scoped narrowly:

```toml
[[rust.method.where]]
id       = "smell-vertical-separation-private-method"
severity = "warn"
expr     = "visibility != 'private' OR count(in_refs, source.parent = parent) = 0 OR line_gap_after(min(in_refs, start_line), start_line) <= 40"
message  = "Private method `{name}` is far from its first local caller."

[[default.local.where]]
id       = "smell-vertical-separation-local"
severity = "warn"
expr     = "count(in_refs) = 0 OR line_gap_after(end_line, min(in_refs, start_line)) <= 8"
message  = "Local value `{name}` is declared far from its first read."
```

The exact syntax above is illustrative. The important semantic need is:

- def-scope numeric projections: `start_line`, `end_line`, `start_byte`,
  `end_byte`;
- ref-scope numeric projections for the reference occurrence:
  `start_line`, `end_line`, `start_byte`, `end_byte`;
- numeric aggregation over `in_refs` and `out_refs`, for example
  `min(in_refs, start_line)`;
- a small, readable way to compare positions with tolerance, either general
  arithmetic or a dedicated helper such as `line_gap_after(a, b)`.

The larger level is a layout predicate that evaluates an owner as a whole:

```toml
[[rust.shape.type.where]]
id       = "smell-vertical-layout"
severity = "warn"
expr     = "vertical_layout(shape:callable, public_first, private_after_first_use, max_gap = 40)"
message  = "Callable layout under `{name}` does not match first-use order."
```

This is intentionally domain-specific. Instead of making users encode a full
sorting algorithm in the DSL, the predicate can compute:

1. the current order of direct child callables by declaration position;
2. the ideal order:
   - public/protected/package-visible callables first, preserving their
     current relative order;
   - private callables placed after the first same-owner callable that refers
     to them;
   - ties broken by current declaration order, so the suggestion is stable;
3. the distance penalty between current and ideal layout.

The violation explanation should carry structured evidence, for example:

```text
current:  parse -> parse_token -> render -> render_row
ideal:    parse -> parse_token -> render -> render_row
move:     helper `parse_token` closer to first caller `parse` (L18 -> L64, gap 46)
```

For JSON output, the same information should be available as a small evidence
object rather than only as prose:

```json
{
  "layout": {
    "domain": "shape:callable",
    "current": ["parse", "render", "parse_token", "render_row"],
    "ideal": ["parse", "parse_token", "render", "render_row"],
    "moves": [
      {
        "symbol": "parse_token",
        "first_use": "parse",
        "from_line": 64,
        "ideal_after_line": 18,
        "gap": 46
      }
    ]
  }
}
```

That implies a small extension to `Violation`: keep the existing `message`
and `explanation`, but add optional machine-readable `evidence`. Text output
can render the top one or two moves; JSON/report consumers can inspect the
whole layout.

## Semantics

For private helpers:

- Anchor the rule on callable defs with private visibility.
- Consider only incoming refs whose source owner is the same type/module as
  the target owner.
- Ignore helpers with no local incoming refs; dead-code rules can handle
  those separately.
- Warn when the helper starts too far below the first local caller.
- Optionally add a stricter variant later: the helper must appear after its
  first caller, not before the public surface.

For owner-level layout:

- Anchor the rule on an owner (`shape:type`, `module`, or eventually any
  scope-opening def).
- Select a declaration domain such as `shape:callable`, `method`, `fn`,
  `field`, or `local`.
- Compare current order to an ideal order computed by a named policy.
- Emit one violation on the owner, not one violation per child, when the
  ordering is meaningfully different.
- Include the suggested order and the most actionable move in the
  explanation.

For local values:

- Anchor the rule on `local` defs.
- Use the first `reads` ref to the local symbol.
- Warn when declaration and first read are separated by more than a small
  threshold.
- Allow same-line declaration/use and short guard/setup blocks.

## Implementation shape

The data path already exists:

- `code_moniker_core::core::code_graph::DefRecord.position`
- `code_moniker_core::core::code_graph::RefRecord.position`
- `code_moniker_workspace::lines::line_range`

The DSL work would be localized around:

- `crates/cli/src/check/expr/ast.rs`: add numeric `Lhs` variants and parser
  names for position projections.
- `crates/cli/src/check/eval/mod.rs`: resolve the new projections from the
  current def or ref position.
- `crates/cli/src/check/eval/collection.rs` and pair projection handling:
  allow collection paths like `in_refs.start_line`.
- `docs/cli/check-dsl.md`: document position projections and examples.
- Focused eval tests for def position, ref position, and aggregate use over
  `in_refs`.

For `vertical_layout(...)`, add a dedicated predicate path rather than
pretending it is a normal boolean atom:

- Parser: recognize `vertical_layout(<domain>, <policy>...)` as a predicate.
- Eval: collect direct children from the domain, sort by position, compute
  first-use relationships from `in_refs`, and return pass/fail plus evidence.
- Output model: add optional evidence/details to violations so layout
  suggestions can be rendered and serialized without stuffing everything into
  `{expected}`.
- Tests: use tiny fixtures with public methods, private helpers, unused
  helpers, same-line/same-block helpers, and tie cases.

If general arithmetic is still intentionally absent, prefer a tiny
domain-specific helper over adding all arithmetic at once. G10 needs
distance comparison more than arbitrary math.

## Policy sketch

Initial policies can be deliberately few:

- `public_first`: visible API before private implementation details.
- `private_after_first_use`: private child appears shortly after the first
  same-owner ref to it.
- `locals_before_first_read`: local declarations stay close to first read.
- `preserve_groups`: never move a child across a different visibility group
  unless a stricter policy says so.

This keeps the DSL expressive enough for G10 without becoming a layout
programming language.

## Review posture

This should not become an error rule immediately. Vertical separation is a
readability signal with legitimate exceptions: trait impl grouping, generated
style, protocol tables, tests, and small modules can all be better served by a
different order. Start with `severity = "warn"`, cap broad runs with
`--max-violations`, inspect whether findings point to real navigation pain,
then tune thresholds by language and module style.
