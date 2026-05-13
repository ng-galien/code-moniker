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

`code-moniker` extracts a symbol graph from source code.

It gives you two surfaces over the same model:

- a CLI for inspecting code and enforcing architecture rules in hooks or CI;
- a PostgreSQL extension for storing and querying symbol graphs with SQL.

Supported languages: TypeScript / JavaScript / TSX / JSX, Rust, Java,
Python, Go, C#, SQL, and PL/pgSQL.

## What it is for

Use `code-moniker` when text search is too weak because the question is
about symbols and relationships:

- Which definitions live under `src/domain/`?
- Does domain code import infrastructure code?
- Which classes implement a port?
- Which refs point at a symbol family, even when the final segment kind
  differs across import and definition sites?
- Can this rule run after every edit, before commit, or in CI?

The unit of identity is a `moniker`: a URI-like path made of typed
segments.

```text
code+moniker://app/lang:ts/dir:src/dir:domain/module:order/class:OrderEntity
```

The graph then stores defs and refs between those monikers: calls,
imports, inheritance, implemented interfaces, type usage, annotations,
and related language-specific edges.

## Install

Install the standalone CLI:

```sh
cargo install code-moniker
```

Or install the latest `main`:

```sh
cargo install --git https://github.com/ng-galien/code-moniker code-moniker
```

From a local checkout:

```sh
cargo install --path crates/cli
```

## First CLI run

Inspect a file:

```sh
code-moniker extract src/order.ts --format tree
```

Inspect a directory:

```sh
code-moniker extract src/
```

Filter by kind or shape:

```sh
code-moniker extract src/ --shape callable
code-moniker extract src/ --kind class,interface
```

Run the linter:

```sh
code-moniker check src/
```

Exit codes:

| Code | Meaning |
| ---- | ------- |
| `0`  | no violations |
| `1`  | at least one violation |
| `2`  | usage or configuration error |

## Configure rules

`code-moniker check` loads embedded defaults first. If a
`.code-moniker.toml` file exists, it is merged on top.

```toml
[[refs.where]]
id      = "domain-no-infra"
expr    = "source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'"
message = "Domain code must not depend on infrastructure."

[[ts.class.where]]
id      = "no-god-class"
expr    = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` exceeds the class budget."

[[ts.interface.where]]
id   = "repository-lives-in-domain"
expr = "name =~ Repository$ => moniker ~ '**/dir:domain/**'"
```

Rules evaluate symbols and refs, not source text. The path pattern must
match the moniker encoding produced by the extractor. Check one file when
in doubt:

```sh
code-moniker extract src/order.ts --format json
```

## PostgreSQL extension

The extension installs native `moniker` and `code_graph` types, extractors,
accessors, and indexes. It owns no application tables.

```sql
CREATE EXTENSION code_moniker;
SET search_path = code_moniker, public;

SELECT extract_typescript(
  'src/util.ts',
  'export class Util { run() { return 1; } }',
  'code+moniker://app'::moniker
);
```

Example indexed query:

```sql
SELECT id
FROM module
WHERE graph_root(graph) <@ 'code+moniker://app/lang:ts/dir:domain'::moniker;
```

Install and usage details live in the PostgreSQL docs.

## Documentation

Start with the page that matches the task:

| Task | Page |
| ---- | ---- |
| Inspect symbols from the CLI | [Extract](docs/cli/extract.md) |
| Lint a repository with rules | [Check](docs/cli/check.md) |
| Write rule expressions | [Rule DSL](docs/cli/check-dsl.md) |
| Wire the linter into hooks or CI | [Agent harness](docs/cli/agent-harness.md) |
| Query graphs in PostgreSQL | [Postgres usage](docs/postgres/usage.md) |
| Look up SQL functions and operators | [Postgres reference](docs/postgres/reference.md) |
| Understand moniker URI syntax | [Moniker URI](docs/design/moniker-uri.md) |
| Read the full model | [Design spec](docs/design/spec.md) |
| Build or contribute | [Contributing](CONTRIBUTING.md) |

Full index: [docs/](docs/README.md).

## Performance

The CLI is designed for hooks and CI. Project scans are parallel; per-file
checks are bounded enough for edit hooks. Measurements and reproduction
commands are in [Performance](docs/perf.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE),
at your option. Contributions are accepted under the same terms.
