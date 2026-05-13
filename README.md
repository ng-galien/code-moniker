<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo-dark.svg">
    <img src="docs/logo-light.svg" alt="code-moniker" width="300">
  </picture>
</p>

# code-moniker

[![CI](https://github.com/ng-galien/code-moniker/actions/workflows/ci.yml/badge.svg)](https://github.com/ng-galien/code-moniker/actions/workflows/ci.yml)
[![License: MIT or Apache 2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange)](https://www.rust-lang.org)
[![pgrx](https://img.shields.io/badge/pgrx-0.18-darkgreen)](https://github.com/pgcentralfoundation/pgrx)
[![PostgreSQL](https://img.shields.io/badge/postgresql-17-336791)](https://www.postgresql.org)

`code-moniker` makes the symbol graph queryable. Two surfaces, one
extractor:

- a **standalone CLI** that lints projects against a declarative rule
  pack — usable as an agent guardrail, a pre-commit gate, or a CI job;
- a **PostgreSQL extension** that exposes the same graph as native
  SQL types (`moniker`, `code_graph`) with an indexed algebra.

No index to maintain, no daemon — the linter runs on any checkout
without setup; benchmarks live in [`docs/perf.md`](docs/perf.md).
Supported languages: TypeScript / JavaScript / TSX / JSX, Rust, Java,
Python, Go, C#, SQL, PL/pgSQL.

## Why this exists

**SCIP / LSIF / tree-sitter-graph** emit symbol graphs as static
files — you bolt your own consumer on top to query them. **Semgrep
CE, ast-grep**, and local syntax-pattern matchers give you a query
language but match syntax, not a symbol graph, so cross-file refs
and layering constraints (`domain/` must not depend on
`infrastructure/`) stay out of reach as primitives.

`code-moniker` bakes structural context into the symbol identity.
The AST of `class OrderEntity` under `src/domain/order/`
materialises like this (scanning with `src/` as root):

```
    // src/domain/order.ts
    class OrderEntity { save(r: OrderRepo) {…} }

                            │ extract
                            ▼

    moniker (identity + structural path, one per def):
      ◆ code+moniker://app/lang:ts/dir:domain/module:order/class:OrderEntity
                                    └────────┘
                                       layering anchor — pattern-matchable
      ◆ …/class:OrderEntity/method:save(OrderRepo)

    code_graph (edges between monikers):
      …/method:save  ── uses_type ──▶  …/dir:domain/module:repo/interface:OrderRepo
```

The moniker URI carries identity and structural path; `code_graph`
carries the relations (calls, imports, implements, extends,
uses_type) between monikers. A rule like
`source ~ '**/dir:domain/**' => target ~ '**/dir:domain/**'`
becomes a one-liner the linter enforces statelessly, file by file.

The Postgres extension is this model ported into a database.
`moniker` and `code_graph` become native SQL types; the algebra
(`<@` for subtree, `?=` for `bind_match` cross-file resolution,
`@>` for ancestry) becomes SQL operators backed by GiST and GIN
indexes. The symbol graph now sits next to your domain tables and
joins with them in one query:

```sql
-- Which deployments in the last week touched code under dir:domain/?
SELECT d.id, d.deployed_at, m.source_uri
FROM module m
JOIN deployment d ON d.path = m.source_uri
WHERE graph_root(m.graph) <@ 'code+moniker://app/lang:ts/dir:domain'::moniker
  AND d.deployed_at > now() - interval '7 days';
```

## Install

CLI (standalone, no Postgres needed):

```sh
cargo install --git https://github.com/ng-galien/code-moniker --features cli code-moniker
```

From a local clone:

```sh
cargo install --path . --features cli
```

Postgres extension (PG17 via pgrx; Docker variant in [`docs/use-in-postgres.md`](docs/use-in-postgres.md)):

```sh
cargo install --locked cargo-pgrx
cargo pgrx init --pg17 download
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
```

Then `CREATE EXTENSION code_moniker;` in any PG17 database.

## CLI — `code-moniker check`

```toml
# .code-moniker.toml
[[refs.where]]
id   = "domain-no-infra"
expr = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"

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

The bare `code-moniker <path>` form (no `check`) is a probe: a single
file emits its full graph (TSV / JSON), a directory emits a per-file
summary, and `--kind` / `--where` turn it into a filtered cross-tree
query. Useful for ad-hoc exploration without writing a rule pack.

→ [docs/use-as-agent-harness.md](docs/use-as-agent-harness.md) ·
[docs/cli-extract.md](docs/cli-extract.md)

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
defs and refs. Cross-file linkage is a single indexed JOIN on `?=`
(`bind_match`). The extension owns no tables — types, operators,
and pure functions only.

→ [docs/use-in-postgres.md](docs/use-in-postgres.md)

## Doc map

| Goal                                                                | Read                                                              |
|---------------------------------------------------------------------|-------------------------------------------------------------------|
| Lint a project, gate an agent, guard pre-commit / CI                | [docs/use-as-agent-harness.md](docs/use-as-agent-harness.md)      |
| Index a corpus in Postgres for cross-file queries                   | [docs/use-in-postgres.md](docs/use-in-postgres.md)                |
| CLI reference (per-file probe, project linter, rule DSL)            | [docs/README.md](docs/README.md)                                  |
| Add a language, change the SQL surface, build & test                | [CONTRIBUTING.md](CONTRIBUTING.md) · [docs/design/spec.md](docs/design/spec.md) |

## Surface

- Types: `moniker`, `code_graph`.
- Operators: `=`, `?=` (`bind_match`), `<` / `<=` / `>` / `>=`,
  `<@` / `@>`, `||` (compose child).
- Indexes: btree / hash / GiST on `moniker`, GIN over `moniker[]`.
- Extractors: `extract_typescript`, `extract_rust`, `extract_java`,
  `extract_python`, `extract_go`, `extract_csharp`,
  `extract_plpgsql`. Manifest parsers: `extract_cargo`,
  `extract_package_json`, `extract_pom_xml`, `extract_pyproject`,
  `extract_go_mod`, `extract_csproj`.
- Constructors for synthetic graphs: `code_graph_declare(jsonb)` /
  `code_graph_to_spec(code_graph)`.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE),
at your option. Contributions are accepted under the same terms.
