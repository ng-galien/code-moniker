# `code-moniker` documentation

`code-moniker` is two things sharing one extractor core:

- a **standalone CLI** that turns source files into a symbol graph and
  lints projects against a declarative rule pack;
- a **PostgreSQL extension** that exposes the same graph as native SQL
  types (`moniker`, `code_graph`) with an indexed algebra.

Start with the guide matching your use case.

## Using `code-moniker`

| You want to…                                                                                  | Read                                                            |
|-----------------------------------------------------------------------------------------------|-----------------------------------------------------------------|
| Gate an agent (Claude Code, etc.), pre-commit, or CI against architecture violations          | [`use-as-agent-harness.md`](use-as-agent-harness.md)            |
| Index a code corpus inside Postgres for cross-file queries                                     | [`use-in-postgres.md`](use-in-postgres.md)                      |
| Probe a single source file from the shell (moniker by moniker, JSON / TSV out)                | [`cli-extract.md`](cli-extract.md)                              |
| Reference for the `check` subcommand: config merge, suppressions, output format               | [`cli-check.md`](cli-check.md)                                  |
| Write or extend rules — grammar, scopes, quantifiers, path patterns                           | [`check-dsl.md`](check-dsl.md)                                  |
| Benchmarks: single-file latency and project-scan throughput                                    | [`perf.md`](perf.md)                                            |

## Design and reference

Deeper, for consumers building on the SQL surface or contributors
adding a language.

| Topic                                                                                          | Read                                                            |
|------------------------------------------------------------------------------------------------|-----------------------------------------------------------------|
| Conceptual model, full SQL surface, implementation phases                                      | [`design/spec.md`](design/spec.md)                              |
| Moniker URI grammar, operators (`=`, `bind_match`, `<@`, `@>`, `~`), escaping                  | [`design/moniker-uri.md`](design/moniker-uri.md)                |
| JSON Schema for the declarative graph constructor (`code_graph_declare`)                       | [`declare_schema.json`](declare_schema.json)                    |

## Contributing

| Topic                                                                                          | Read                                                            |
|------------------------------------------------------------------------------------------------|-----------------------------------------------------------------|
| Build, test, extractor skeleton, pgrx invariants                                                | [`../CONTRIBUTING.md`](../CONTRIBUTING.md)                      |
