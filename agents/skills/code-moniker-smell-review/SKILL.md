---
name: code-moniker-smell-review
description: Review a repository with code-moniker's local and workspace check DSL for Fowler and Lanza-Marinescu code smells. Use when Codex needs to write, validate, or run project-specific warning-severity smell rules, distinguish executable local or indexed DSL checks from AST/history checks, triage check output, or plan follow-up evolutions for missing smell-detection operators.
---

# Code Moniker Smell Review

## Operating Mode

Use the check DSL first. Treat smell rules as heuristics, not hard
architecture gates, unless the user explicitly asks to enforce them.

Default every smell rule to `severity = "warn"`. Warning rules should
surface review candidates without making `code-moniker check` exit with a
failure by themselves.

For code-moniker itself, prefer these repo-local files:

- `docs/cli/code-smell-review.md` for the documented model and boundaries.
- `.code-moniker.toml` for project-specific smell warnings and profiles.
- `docs/cli/check-dsl.md` for exact grammar and projection semantics.

## Workflow

1. Inspect the target repository's language mix and existing rule overlay:
   `code-moniker rules show . --report` when available, otherwise inspect
   `.code-moniker.toml` and `code-moniker.fragment.toml` files.
2. Before reviewing the local delta, inventory every mechanism it introduces
   or changes: state machine, timeout, retry loop, cache, parser, classifier,
   launcher, and policy. Use symbolic search/usages across the full repository,
   name the canonical owner and consumers, and flag a parallel owner even when
   its implementation is not a textual clone.
3. Select checks the DSL can execute: direct child defs,
   `out_refs`, `in_refs`, local metrics, collection algebra, entropy,
   mode, percentile, `cv`, and `gini`; or `workspace.group` member-line
   distributions over the current full index.
4. Keep out-of-scope smells out of the CLI ruleset: change-history smells,
   clone detection, transitive message chains, reaching-defs, z-scores,
   and arbitrary corpus projections still belong to later DSL, SQL/PG, or
   extractor work.
5. Validate rules before running a broad review:
   `code-moniker rules show . --profile smells`.
6. Run the review as warnings:
   `code-moniker check <repo-root> --profile smells --report --max-violations 50`.
7. Triage output by smell family. Report findings as review candidates with
   file/line evidence, not as proof of incorrectness.
8. Report a separate `Mechanism reuse / duplication` verdict with the
   repository-wide evidence. A clean DSL result is not evidence that semantic
   duplication is absent.
9. For a rule that would be useful but cannot be expressed, create an
   `evolutions/` note instead of forcing an invalid or misleading TOML rule.

## Rule Guidance

Use shape scopes for broad polyglot checks:

```toml
[[shape.callable.where]]
id       = "smell-long-callable"
severity = "warn"
expr     = "lines <= 120"
```

Use type scopes for local OO distribution checks:

```toml
[[shape.type.where]]
id       = "smell-harmonious-method-size"
severity = "warn"
expr     = "count(shape:callable) >= 5 => cv(shape:callable, lines) <= 0.6"
```

Use implication guards to avoid flagging tiny symbols:

```toml
[[shape.callable.where]]
id       = "smell-feature-envy-local"
severity = "warn"
expr     = "count(out_refs) >= 5 => mode(out_refs, target.parent) = source.parent"
```

Use `workspace.group` for cross-file line-distribution candidates already
available in the hot inventory:

```toml
[[workspace.group.where]]
id        = "smell-package-size-disharmony"
severity  = "warn"
members   = "shape = 'type'"
group_by  = ["lang", "segment('package')"]
expr      = "count(member) >= 8 => gini(member, lines) <= 0.65"
message   = "Uneven type sizes in {group}: {observations}"
```

Keep the sample-size guard. `lines` is the inclusive extracted symbol span.
The group evaluator requires a valid line range for every selected member and
reports coverage instead of folding a biased subset. Boolean order does not
change the verdict: a decisive known `AND`/`OR` operand wins, otherwise the
unavailable statistic remains fail-closed.

Use segment regexes to keep path-pattern rules compact and structural:

```toml
[aliases]
adapter_layer = "target ~ '**/*:/^(adapter|infrastructure)$/**'"
```

Prefer `*:/regex/` when a rule repeats the same `source ~`, `target ~`,
`uri ~`, or `moniker ~` path with only the segment name changing. Do not
replace semantically different alternatives with a regex just to shorten a
rule.

Do not write rules that rely on unsupported arithmetic, AST control-flow
shape, arbitrary corpus projections, or history. Capture those as evolutions.

## Reference

Read `references/local-smell-coverage.md` when deciding whether a smell is
covered by the local DSL or needs a follow-up evolution.
