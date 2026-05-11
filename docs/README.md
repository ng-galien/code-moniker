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
| Gate an agent (Claude Code, etc.), pre-commit, or CI against architecture violations          | [`USE_AS_AGENT_HARNESS.md`](USE_AS_AGENT_HARNESS.md)            |
| Index a code corpus inside Postgres for cross-file queries (the ESAC use case)                | [`USE_IN_POSTGRES.md`](USE_IN_POSTGRES.md)                      |
| Probe a single source file from the shell (moniker by moniker, JSON / TSV out)                | [`CLI_EXTRACT.md`](CLI_EXTRACT.md)                              |
| Reference for the `check` subcommand: config merge, suppressions, output format               | [`CLI_CHECK.md`](CLI_CHECK.md)                                  |
| Write or extend rules — grammar, scopes, quantifiers, path patterns                           | [`CHECK_DSL.md`](CHECK_DSL.md)                                  |

## Design and reference

Deeper, for consumers building on the SQL surface or contributors
adding a language.

| Topic                                                                                          | Read                                                            |
|------------------------------------------------------------------------------------------------|-----------------------------------------------------------------|
| Conceptual model, full SQL surface, implementation phases                                      | [`design/SPEC.md`](design/SPEC.md)                              |
| Moniker URI grammar, operators (`=`, `bind_match`, `<@`, `@>`, `~`), escaping                  | [`design/MONIKER_URI.md`](design/MONIKER_URI.md)                |
| JSON Schema for the declarative graph constructor (`code_graph_declare`)                       | [`declare_schema.json`](declare_schema.json)                    |

## Contributing

| Topic                                                                                          | Read                                                            |
|------------------------------------------------------------------------------------------------|-----------------------------------------------------------------|
| Build, test, dogfood loops; per-language extractor skeleton; pgrx invariants                   | [`../CONTRIBUTING.md`](../CONTRIBUTING.md)                      |
