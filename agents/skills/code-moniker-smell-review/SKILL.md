---
name: code-moniker-smell-review
description: Review a repository with code-moniker's local check DSL for Fowler and Lanza-Marinescu code smells. Use when Codex needs to write, validate, or run project-specific warning-severity smell rules, distinguish executable local DSL checks from AST/corpus/history checks, triage check output, or plan follow-up evolutions for missing smell-detection operators.
---

# Code Moniker Smell Review

## Operating Mode

Use the local CLI DSL first. Treat smell rules as heuristics, not hard
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
2. Select only local checks the DSL can execute: direct child defs,
   `out_refs`, `in_refs`, local metrics, collection algebra, entropy,
   mode, percentile, `cv`, and `gini`.
3. Keep out-of-scope smells out of the CLI ruleset: change-history smells,
   clone detection, transitive message chains, reaching-defs, and
   corpus-wide statistical baselines belong to later SQL/PG or extractor work.
4. Validate rules before running a broad review:
   `code-moniker rules show . --profile smells`.
5. Run the review as warnings:
   `code-moniker check <repo-root> --profile smells --report --max-violations 50`.
6. Triage output by smell family. Report findings as review candidates with
   file/line evidence, not as proof of incorrectness.
7. For a rule that would be useful but cannot be expressed, create an
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
shape, cross-file closure, or history. Capture those as evolutions.

## Reference

Read `references/local-smell-coverage.md` when deciding whether a smell is
covered by the local DSL or needs a follow-up evolution.
