# code-moniker

[![CI](https://github.com/ng-galien/code-moniker/actions/workflows/ci.yml/badge.svg)](https://github.com/ng-galien/code-moniker/actions/workflows/ci.yml)
[![License: MIT or Apache 2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)](https://www.rust-lang.org)
[![pgrx](https://img.shields.io/badge/pgrx-0.18-darkgreen)](https://github.com/pgcentralfoundation/pgrx)
[![PostgreSQL](https://img.shields.io/badge/postgresql-17-336791)](https://www.postgresql.org)

Symbol identity and a symbol graph for source code, in two shapes that
share one extractor:

- a **standalone CLI** that lints projects against a declarative rule
  pack — usable as an agent guardrail, a pre-commit gate, or a CI job;
- a **PostgreSQL extension** that exposes the same graph as native
  SQL types (`moniker`, `code_graph`) with an indexed algebra.

Supported languages: TypeScript / JavaScript / TSX / JSX, Rust, Java,
Python, Go, C#, PL/pgSQL.

## CLI — `code-moniker check`

```toml
# .code-moniker.toml
[[refs.where]]
id   = "domain-no-infra"
expr = "source ~ '**/module:domain/**' => NOT target ~ '**/module:infrastructure/**'"

[[ts.class.where]]
id   = "no-god-class"
expr = "count(method) <= 20 AND all(method, lines <= 60)"
```

```sh
$ code-moniker check src/
src/domain/order.ts:L42-L88 [ts.class.no-god-class] class `Order` fails `count(method) <= 20`
  → Class `Order` is too wide (24).
1 violation(s) across 1 file(s) (47 scanned).
$ echo $?
1
```

Rules talk about symbols and their relations (calls, imports,
inheritance, layering, naming), not just syntax. Exit 1 is the signal
for `PostToolUse` hooks, pre-commit, and CI.

→ [docs/USE_AS_AGENT_HARNESS.md](docs/USE_AS_AGENT_HARNESS.md)

## Postgres extension — `extract_<lang>` + indexed algebra

```sql
CREATE EXTENSION code_moniker;

SELECT extract_typescript(
  'src/util.ts',
  'export class Util { run() { return 1; } }',
  'code+moniker://app'::moniker
);

SELECT 'code+moniker://app/lang:ts/dir:src/module:util/class:Util'::moniker
    <@ 'code+moniker://app/lang:ts'::moniker;   -- subtree containment, GiST-indexed
```

`moniker` carries node identity; `code_graph` carries a module's
defs / refs / containment tree. Cross-file linkage is a single
indexed JOIN on `bind_match`. The extension owns no tables — types,
operators, and pure functions only.

→ [docs/USE_IN_POSTGRES.md](docs/USE_IN_POSTGRES.md)

## Doc map

| Goal                                                                | Read                                                              |
|---------------------------------------------------------------------|-------------------------------------------------------------------|
| Lint a project, gate an agent, guard pre-commit / CI                | [docs/USE_AS_AGENT_HARNESS.md](docs/USE_AS_AGENT_HARNESS.md)      |
| Index a corpus in Postgres for cross-file queries                   | [docs/USE_IN_POSTGRES.md](docs/USE_IN_POSTGRES.md)                |
| CLI reference (per-file probe, project linter, rule DSL)            | [docs/README.md](docs/README.md)                                  |
| Add a language, change the SQL surface, build & test                | [CONTRIBUTING.md](CONTRIBUTING.md) · [docs/design/SPEC.md](docs/design/SPEC.md) |

## Surface

- Types: `moniker`, `moniker_pattern`, `code_graph`.
- Algebra: `=`, `bind_match`, `<@`, `@>`, `||`, `~`.
- Indexes: btree / hash / GiST on `moniker`, GIN over `moniker[]`.
- Extractors: one per supported language, with manifest parsers for
  `Cargo.toml`, `package.json`, `pom.xml`, `pyproject.toml`,
  `go.mod`, `.csproj`.
- Constructors for synthetic graphs (`code_graph_declare` /
  `code_graph_to_spec`) for forward modeling and external libraries.

The extension is stateless: no tables, no triggers, no I/O against
external state. Cross-module resolution is the consumer's responsibility,
performed by a JOIN on `bind_match`. Storage and querying are Postgres'
job; the CLI shares the same extractor core and adds a rule engine
on top.

## Not in scope

- No application schema (tables, RLS, triggers).
- No project-level configuration storage; callers pass anchors and
  presets as arguments.
- No cross-project federation.
- No stack-graph-style dynamic resolution; the moniker must be
  locally determinable from the source.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE),
at your option. Contributions are accepted under the same terms.
