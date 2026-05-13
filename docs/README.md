# Documentation

## CLI — `code-moniker`

- [Extract](cli/extract.md) — dump the moniker graph for a file or directory
- [Check](cli/check.md) — lint a path against `.code-moniker.toml` rules
- [Rule DSL](cli/check-dsl.md) — grammar: scopes, quantifiers, projections, path patterns
- [Discovery](cli/langs.md) — `langs` and `shapes` vocabularies
- [Agent harness](cli/agent-harness.md) — wire `check` into Claude Code hooks, pre-commit, or CI

## PostgreSQL extension

- [Reference](postgres/reference.md) — install, types, operators, accessors, constructors, extractors, indexes
- [Usage](postgres/usage.md) — schema layout, populate, query patterns, binary I/O
- [Declare schema](postgres/declare-schema.json) — JSON Schema for `code_graph_declare`

## Design

- [Spec](design/spec.md) — conceptual model, full SQL surface, per-language contract
- [Moniker URI](design/moniker-uri.md) — URI grammar, operators, escaping

## Other

- [Performance](perf.md) — single-file latency, project-scan throughput, cache impact
- [Contributing](../CONTRIBUTING.md) — build, test, add a language, pgrx invariants
