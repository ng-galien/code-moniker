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
[![Rust](https://img.shields.io/badge/rust-1.86%2B-orange)](https://www.rust-lang.org)
[![pgrx](https://img.shields.io/badge/pgrx-0.18-darkgreen)](https://github.com/pgcentralfoundation/pgrx)
[![PostgreSQL](https://img.shields.io/badge/postgresql-17-336791)](https://www.postgresql.org)

`code-moniker` extracts a symbol graph from source code.

It turns source files into stable symbol identities, then exposes the
same graph through two surfaces:

- a CLI for inspecting code and enforcing architecture rules in hooks or CI;
- a PostgreSQL extension for storing and querying symbol graphs with SQL.

Supported languages: TypeScript / JavaScript / TSX / JSX, Rust, Java,
Python, Go, C#, SQL, and PL/pgSQL.

## At a glance

```mermaid
flowchart LR
  subgraph Input["Inputs"]
    S["Source code<br/>TS, Rust, Java, Python,<br/>Go, C#, SQL"]
    M["Build manifests<br/>Cargo.toml, package.json,<br/>pom.xml, pyproject.toml,<br/>go.mod, csproj"]
  end

  subgraph Model["Extraction model"]
    E["Language extractors"]
    G["Code graph<br/>defs, refs, monikers,<br/>positions, attributes"]
    D["Dependency rows<br/>package monikers"]
  end

  subgraph Tools["Tools"]
    C["CLI<br/>extract, check, manifest"]
    P["PostgreSQL extension<br/>moniker + code_graph types,<br/>SQL extractors, indexes"]
  end

  subgraph Uses["Uses"]
    I["Inspection<br/>tree, json, tsv"]
    R["Architecture rules<br/>hooks, CI, agent harnesses"]
    Q["SQL queries<br/>storage, joins, indexed lookup"]
  end

  S --> E --> G
  M --> D
  G --> C
  G --> P
  D --> C
  C --> I
  C --> R
  P --> Q

  classDef input fill:#eef6ff,stroke:#2f6f9f,color:#0b253a
  classDef model fill:#f1f8f4,stroke:#3a7d4f,color:#0f2a18
  classDef tool fill:#fff6e5,stroke:#9a6b12,color:#332100
  classDef use fill:#f7f1ff,stroke:#6f4aa1,color:#211232
  class S,M input
  class E,G,D model
  class C,P tool
  class I,R,Q use
```

First useful commands:

```sh
code-moniker extract src/order.ts --format tree
code-moniker ui . --cache .code-moniker-cache
code-moniker check src/ --report
code-moniker manifest .
```

## What it is for

Use `code-moniker` when text search is too weak because the question is
about symbols and relationships:

- Which definitions live under `src/domain/`?
- Does domain code import infrastructure code?
- Which classes implement a port?
- Which refs point at a symbol family, even when the final segment kind
  differs across import and definition sites?
- Can this rule run after every edit, before commit, or in CI?

## How extraction works

The unit of identity is a `moniker`: a URI-like path made of typed
segments. Each segment says what the name means, not only where text was
found.

For this file:

```ts
// src/domain/order.ts
export class OrderEntity {
  total() {
    return computeTotal();
  }
}

function computeTotal() {
  return 42;
}
```

`extract` emits definitions such as:

```text
code+moniker://./lang:ts/dir:src/dir:domain/module:order/class:OrderEntity
code+moniker://./lang:ts/dir:src/dir:domain/module:order/function:computeTotal()
```

It also emits refs between those definitions. The call inside
`OrderEntity.total()` points at the `function:computeTotal()` moniker,
so rules and queries can reason over relationships instead of strings.

Common ref kinds include calls, imports, inheritance, implemented
interfaces, type usage, annotations, and language-specific edges. In
project scans, file paths are anchored relative to the scanned root:
`code-moniker extract src/` sees `src/domain/order.ts` as
`dir:domain/module:order`.

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

Open the read-only terminal explorer:

```sh
code-moniker ui . --cache .code-moniker-cache
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
