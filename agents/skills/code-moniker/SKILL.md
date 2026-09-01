---
name: code-moniker
description: >-
  Use Code Moniker to onboard into an unfamiliar project, develop with indexed
  structural evidence, guard executable architectural memory after a change, or
  review relationships and change impact. Use it when callers, callees,
  coupling, ownership, project rules, or workspace-wide structure matter. Do
  not invoke it for routine Git, tests, exact-string lookup, or small known-file
  edits whose contract is already visible.
---

# Code Moniker

Code Moniker combines project-authored architectural testimony with a symbolic
index. This skill defines how an agent works with those surfaces. It does not
teach the product concepts: the executable `rules learn` corpus does.

## Choose a posture

Read only the posture that matches the current responsibility, then follow it
before acting:

| Posture | Use it when | Playbook |
|---|---|---|
| Onboard | Entering an unfamiliar project or rebuilding project context | [postures/onboard.md](postures/onboard.md) |
| Develop | Focusing structural exploration and implementing a change | [postures/develop.md](postures/develop.md) |
| Guard | Reassessing executable architectural memory after a change | [postures/guard.md](postures/guard.md) |
| Review | Evaluating architecture, relationships, risks, or change impact | [postures/review.md](postures/review.md) |

Postures may follow one another. A typical change uses Onboard once, Develop
for the implementation, and Guard after validation. Review remains read-only
unless the user separately authorizes changes.

## Learn from the product

The skill is a router, not a substitute for `rules learn`. Run the relevant
topic before first interpreting or changing that concept in the current task.
Do not skip it because the concept appears familiar, and do not run every topic
when only one is relevant. Re-run a topic when the Code Moniker version or the
relevant learn corpus changed.

| Need | Required command | What it teaches |
|---|---|---|
| Rule structure and expressions | `code-moniker rules learn basics` | Rule blocks, ids, predicates, severity, messages, and rationale |
| Project vocabulary and testimony | `code-moniker rules learn taxonomy` | Patterns, components, natural rule ids, aliases, and diagnostic interpretation |
| Language tags and rule namespaces | `code-moniker rules learn languages` | Language-specific recipes, plus independent TS, TSX, JS, and JSX policy over a shared analysis ecosystem |
| Architectural locations | `code-moniker rules learn paths` | Moniker path patterns and reusable aliases |
| Local architecture files | `code-moniker rules learn fragments` | Fragments, view URIs, merging, and namespaced rule ids |
| Cross-symbol boundaries | `code-moniker rules learn refs` | Reference rules for imports, calls, inheritance, annotations, and layers |
| Child collections | `code-moniker rules learn collections` | Collection predicates and multiset operations over child symbols |
| Iterated symbol sets | `code-moniker rules learn domains` | Domains, descendants, pairs, shapes, and quantifiers |
| Local structural measures | `code-moniker rules learn metrics` | Named metrics bound to the current or iterated symbol |
| Distribution measures | `code-moniker rules learn aggregates` | Numeric aggregates, dispersion, entropy, and mode |
| Moniker relationships | `code-moniker rules learn relations` | Ancestor, descendant, and binding relation operators |
| Structural directives | `code-moniker rules learn directives` | Layout, correlated-existence, and moniker-segment directives |
| Rule-set selection | `code-moniker rules learn profiles` | Defaults, profiles, warning severity, and suppressions |

Language, framework, architecture-pattern, and workspace recipes are discovered
from the executable catalog rather than duplicated in this router. Run
`code-moniker rules learn` for progressive human navigation or
`code-moniker rules learn --format json` for the complete machine-readable
inventory, then load the relevant name or alias with
`code-moniker rules learn <topic>`. Examples
include `java`, `spring`, `java-qualified-types`, `react`, `javascript`, `jsx`,
and `sql`.

If a required learn command is unavailable, report that limitation instead of
reconstructing its semantics from this skill.

## Shared operating contract

- Use Code Moniker only when structural or architectural evidence can change
  the answer. Prefer ordinary repository tools for direct local facts.
- Select one symbolic surface. Prefer available `code_moniker_*` MCP tools;
  use the local CLI when MCP is unavailable. Hooks enforce write-time policy
  and do not replace exploration.
- Verify workspace identity once before the first workspace-wide MCP read and
  again only after roots or the connection change.
- Start compact, narrow, and bounded. Page, broaden, or request source only
  when omitted evidence can change the answer.
- Reuse returned monikers and generated follow-up calls rather than guessing
  identities or reconstructing queries.
- Keep project testimony, indexed facts, architectural interpretation, and
  coverage limits distinct. Attribute a finding only to the surface exercised.
- Stop when the evidence is sufficient. Do not replay the same exploration
  through MCP, CLI, and direct daemon queries.
