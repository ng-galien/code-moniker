<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo-dark.svg">
    <img src="docs/logo-light.svg" alt="code-moniker" width="300">
  </picture>
</p>

# code-moniker

[![CI](https://github.com/ng-galien/code-moniker/actions/workflows/ci.yml/badge.svg)](https://github.com/ng-galien/code-moniker/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/code-moniker.svg?label=code-moniker)](https://crates.io/crates/code-moniker)
[![crates.io](https://img.shields.io/crates/v/code-moniker-core.svg?label=code-moniker-core)](https://crates.io/crates/code-moniker-core)
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
without setup; benchmarks are in [Performance](docs/perf.md).
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

The [moniker URI](docs/design/moniker-uri.md) carries identity and
structural path; the [`code_graph`](docs/design/spec.md#the-code_graph)
carries the relations (calls, imports, implements, extends, uses_type)
between monikers. A [`check` rule](docs/cli/check.md) like
`source ~ '**/dir:domain/**' => target ~ '**/dir:domain/**'`
becomes a one-liner the linter enforces statelessly, file by file.

The Postgres extension is this model ported into a database.
[`moniker` and `code_graph`](docs/postgres/reference.md#types) become
native SQL types; the [indexed algebra](docs/postgres/reference.md#operators)
(`<@` for subtree, `?=` for `bind_match` cross-file resolution, `@>`
for ancestry) becomes SQL operators backed by GiST and GIN indexes.
The symbol graph now sits next to your domain tables and joins with
them in one query:

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
cargo install code-moniker
```

Or from git (latest `main`):

```sh
cargo install --git https://github.com/ng-galien/code-moniker code-moniker
```

Or from a local clone:

```sh
cargo install --path crates/cli
```

Postgres extension (PG17 via pgrx; Docker variant in the [SQL reference](docs/postgres/reference.md)):

```sh
cargo install --locked cargo-pgrx
cargo pgrx init --pg17 download
cargo pgrx install --manifest-path crates/pg/Cargo.toml --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
```

Then `CREATE EXTENSION code_moniker;` in any PG17 database.

## CLI

Two subcommands :

- `code-moniker check <path>` — lint against `.code-moniker.toml` rules.
  Exit 1 on the first violation; output line points to file:line and
  carries the rule id. Plugs into `PostToolUse` hooks, pre-commit, CI.
- `code-moniker extract <path>` — dump the moniker graph (TSV / JSON /
  tree) for a file or directory. `--kind`, `--shape`, `--where` turn it
  into a filtered cross-tree query.

Rules talk about symbols and their relations (calls, imports,
inheritance, layering, naming) — not text.

→ [Check](docs/cli/check.md) · [Extract](docs/cli/extract.md) · [Agent harness](docs/cli/agent-harness.md)

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

→ [Postgres usage](docs/postgres/usage.md)

## Documentation

Full index in the [docs/](docs/README.md) tree. Entry points:

- CLI — [Extract](docs/cli/extract.md), [Check](docs/cli/check.md), [Agent harness](docs/cli/agent-harness.md)
- PostgreSQL — [SQL reference](docs/postgres/reference.md), [Usage](docs/postgres/usage.md)
- Design — [Spec](docs/design/spec.md), [Moniker URI](docs/design/moniker-uri.md)
- [Contributing](CONTRIBUTING.md) — build, test, add a language

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE),
at your option. Contributions are accepted under the same terms.
