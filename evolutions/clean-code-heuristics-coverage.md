# Evolution: Clean Code heuristics coverage

## Context

The maintenance philosophy in `./cc` is Robert Martin's *Smells and
Heuristics*. CLAUDE.md restates its spirit operationally: durable findings
become rules, fragments, and rationales — never prose that rots. That is G27,
*Structure over Convention*, applied to the project itself.

An exploration of the workspace through the MCP surface (`code_moniker_read`,
`code_moniker_symbols`, `code_moniker_rules`) shows code-moniker is already a
largely executable encoding of those heuristics. This note records, in one
place, what is covered, what is missing, and what is desirable — the constat and
the direction, without prescribing how any of it should be coded.

## What is already covered

The active rule set (357 rules) maps onto a large share of `./cc`:

- C1–C4 (comment smells) — `rust.comment.comment-max-lines` (cap 4),
  `rust.comment.no-nested-comments`.
- F1 / G15 (too many arguments, selector arguments) —
  `rust.shape.callable.smell-long-parameter-list`,
  `rust.shape.type.smell-data-clumps-param-names`.
- G10 (vertical separation) — `rust.shape.type.smell-vertical-layout`
  (see `check-dsl-vertical-separation-g10.md`).
- G14 (feature envy) — `rust.shape.callable.smell-feature-envy-local`.
- G30 / G34 (a function does one thing, one level of abstraction) —
  `rust.shape.callable.smell-brain-method`, `rust.shape.callable.max-lines`.
- Brain Class / God Class / Response-For-Class (Lanza-Marinescu metrics) —
  `rust.shape.type.smell-brain-class`, `smell-god-type-local-metrics`,
  `smell-response-for-class`, `smell-large-type`.
- G11 / G24 (consistency, naming conventions) — the whole `name-*` family
  across all seven languages.

Two qualities of the current encoding are worth preserving as deliberate
posture:

- **Honest dogfooding.** Running the `agent` profile over code-moniker itself
  reports real warnings (21 at the time of writing: brain methods in the
  tree-sitter language strategies, response-for-class, low-cohesion modules,
  one vertical-layout). The tool does not exempt itself from its own rules.
- **Staged adoption.** Newer smell rules ship as `severity = "warn"` "while in
  adoption", which lets a signal be observed and calibrated before it gates the
  build. New smell signals should follow the same rollout shape.

## What remains missing

One of Martin's most load-bearing heuristics still has no structural encoding
and is left to external tools or to reviewers.

### G5 — Duplication — done

Martin ranks G5 second only to passing the tests. The rule set covers length,
coupling, fan-out, cohesion, and naming, but nothing flags "this callable
already exists, nearly identically, next door". The constat is that the
project's own index already surfaces the duplication — the data is present, only
the rule is absent.

Within `crates/cli/src/mcp/tools/`, the symbol index lists, at identical
signatures, helpers copied across sibling tool modules:

- `is_workspace_uri(uri, scheme)` — three copies: `read.rs:228`,
  `symbols.rs:178`, `usages.rs:167`.
- `normalize_workspace_uri(scheme, request_uri)` — two copies: `read.rs:386`,
  `symbols.rs:538`.
- `line_suffix(symbol)` — two copies: `symbols.rs:531`, `usages.rs:439`.
- The near-twin trio `append_read_next_call` / `append_symbols_next_call` /
  `append_rules_next_call` — same skeleton, six parameters each, one per tool.

These are not deep token clones; they are the same name and arity repeated
across sibling scopes — the duplication that maps cleanly onto "extract a shared
helper". That `smell-data-clumps-param-names` already reasons over repeated name
groups is evidence this class of repetition is within reach of the current
model.

Desirable: a structural duplication signal that treats repetition of the same
callable shape across sibling scopes as the primary cue; that points at the set
of duplicate sites together rather than emitting N independent warnings; and
that is scoped narrowly enough to leave legitimate repetition alone (trait-impl
methods, test fixtures, intentional parallel structure). The strongest argument
for the feature is self-application: a tool that catches the duplication in its
own MCP surface is its own best demonstration.

Done 2026-06-01: first executable slice implemented through the generic
`descendants(<kind>|shape:<shape>)` domain combinator and the existing lazy
requirement resolver, not through a single-purpose duplication DSL primitive.
The MCP fragment now self-applies G5 with pairwise duplicate free-function name
detection over `pairs(descendants(fn))`, evaluated from the `mcp::tools` root
with lazy cross-file descendants.

### F4 / G9 — Dead code

Detection of unreferenced code currently relies on `cargo clippy`'s `dead_code`
lint — an external tool — even though the symbolic usage graph already knows
incoming-reference counts. A recent session removed a method (`RuleSeverity::as_str`)
that clippy flagged, not code-moniker.

Desirable: a native signal for a navigable, non-public symbol with zero incoming
references, so F4 sits under the project's own structure instead of a third-party
lint. The exceptions that must stay quiet are well understood — public API kept
for external consumers, trait requirements, test helpers — which argues for a
narrow scope and `warn` severity at first.

### G27 — Single-consumer module boundaries

G27, *Structure over Convention*, also applies to module ownership. A top-level
module that is only consumed by one other top-level module is often not a real
boundary; it is an implementation detail waiting to be folded under its
consumer, unless it has an explicit architectural reason to stay visible.

The recent CLI/MCP module review showed the useful signal:

- `format` was only consumed by `extract` and was folded into
  `extract::format`.
- `perf` was only consumed by `ui` and was folded into `ui::perf`.
- MCP's `lmnav` helper module had one server consumer and was folded into
  `mcp::server`.
- `ui`, `views`, command entry modules, MCP `context`, and MCP `tools` were kept
  as deliberate top-level boundaries.

The DSL can express pieces of this rule. It can identify Rust modules and can
count distinct incoming owners with collection algebra, for example
`size(unique(in_refs.source.parent)) = 1`. That is not enough to encode the
rule honestly. `source.parent` is only the direct parent, not the top-level
consumer, and `lib.rs` module declarations create incoming refs that must be
ignored. The DSL can filter `count(...)`, but it cannot currently build
`unique(...)` over a filtered collection.

Desirable: either a stable ancestor projection such as
`source.ancestor(module, depth = 5)` / `source.top_level_parent`, or filtered
collection expressions such as
`unique(in_refs where NOT $cli_dispatch_surface, source.top_level_parent)`.
With that, a warning-level rule could flag accidental single-consumer top-level
modules while allowlisting intentional boundaries (`ui`, `views`, command
modules, shared MCP support). Until then, a broad rule would be too noisy for
`agent` because it can only approximate the ownership relation.

## Cross-cutting observations

Surfaced by the same exploration, recorded as intent rather than tasks.

### The MCP `next:` affordance is a design asset

Every MCP tool response ends with a `next:` section proposing scoped, paged
follow-up calls. The tool teaches the consuming agent how to continue. This is a
genuine strength of the surface and is worth stating as an explicit design
principle so it is preserved in any future tool or output, not treated as
incidental formatting.

### Long-signature consistency inside the MCP surface (G11 + F1)

The `usages` tool already groups its parameters into context types
(`UsageQuery`, `UsageCall`, `UsageLookup`). The sibling tools `read`, `symbols`,
and `rules` still thread five- and six-parameter render/next-call signatures
(`render_symbols_lmnav` with six parameters, `append_rules_next_call` with six).
The remedy already exists in the codebase but was not generalized — an internal
inconsistency (G11), and exactly the wide signature `smell-long-parameter-list`
exists to discourage. That rule reports zero violations here, so its threshold
currently sits above the project's own widest signatures. Desirable end state:
uniform parameter grouping across the four tools, with the long-parameter-list
signal calibrated tightly enough to catch the next regression rather than
describe an absence.

### Build and test as one step (E1 / E2)

Martin E1/E2: building and testing should each be a single trivial step. Recent
work on the `dev` profile (removing `panic = "abort"`, which forced a full graph
rebuild between `run` and `test`) and on avoiding feature-set flip-flops moved
the build toward E1. The test gate described in CLAUDE.md is still a multi-step
sequence — the E2 smell. A single entry point that runs the full gate in a
stable target directory is the desirable direction; the multi-command gate is
the current friction.

## Posture

None of the gaps should arrive as error-level rules. Duplication, dead-code, and
tighter parameter signals all have legitimate exceptions. The consistent
precedent is: introduce as `warn`, observe on the project's own code, calibrate
thresholds per language and module style, and only then consider promotion.
