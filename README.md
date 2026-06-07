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

`code-moniker` extracts a symbol graph from source code.

It turns source files into stable symbol identities for inspecting code and
enforcing architecture rules in hooks or CI.

Supported languages: TypeScript / JavaScript / TSX / JSX, Rust, Java,
Python, Go, C#, SQL, and PL/pgSQL.

Extractor maturity is uneven by design. `code-moniker` is a fast symbol graph
extractor, not a replacement for each language compiler or type checker.

| Language | Maturity | Honest limit |
| -------- | -------- | ------------ |
| TypeScript / JavaScript | Good | No TypeScript compiler type-checking. |
| Java | Good | No `javac` semantic model. |
| Rust | Good | No macro expansion or rustc name resolution. |
| C# | Usable | No Roslyn semantic model. |
| Python | Usable | Dynamic runtime behaviour is best-effort. |
| Go | Usable | No `go/types` semantic pass. |
| SQL / PLpgSQL | Focused | Narrow dialect and no catalog-aware planner semantics. |
| C | Planned | Not extracted today. |

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
  end

  subgraph Uses["Uses"]
    I["Inspection<br/>tree, json, tsv"]
    R["Architecture rules<br/>hooks, CI, agent harnesses"]
  end

  S --> E --> G
  M --> D
  G --> C
  D --> C
  C --> I
  C --> R

  classDef input fill:#eef6ff,stroke:#2f6f9f,color:#0b253a
  classDef model fill:#f1f8f4,stroke:#3a7d4f,color:#0f2a18
  classDef tool fill:#fff6e5,stroke:#9a6b12,color:#332100
  classDef use fill:#f7f1ff,stroke:#6f4aa1,color:#211232
  class S,M input
  class E,G,D model
  class C tool
  class I,R use
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

## Agentic development

### Challenge

Agentic development needs a stable contract between the repository and
the model. In practice, that contract is usually carried as prose:
`AGENTS.md`, prompt reminders, review comments, architecture notes, or
grep snippets. The agent must read it, keep it in context, and spend
extra turns validating boundaries that the repository could enforce
directly.

That approach breaks down in predictable ways: prompts can be missed,
grep only matches text, and review passes surface violations after the
diff already exists. In modular monorepos, agents may widen their write
scope while trying to be useful. In code bodies, they may leave narrative
comments about micro-decisions, temporary reasoning, or AI-generated
provenance, turning the code itself into noisy context for future
sessions.

### Executable contract

`code-moniker check` encodes that contract as rules over symbols, refs,
paths, and comments. Run it after writes, before commit, or in CI, and a
failure becomes a concrete repair target before the agent treats the task
as done.

| Agent overhead | Executable guardrail |
| -------------- | -------------------- |
| Long prompt rules | keep repository invariants in `.code-moniker.toml` |
| Grep-based sanity checks | evaluate symbol and reference relationships |
| Review agents for known rules | fail fast in the edit hook |
| Repeated inspect-then-fix turns | return concrete violations after each write |
| Unbounded monorepo edits | enforce write scope by module, package, or owner boundary |
| Architecture drift | block forbidden refs, imports, and layer crossings |
| Ownership ambiguity | require symbols to live under the expected path |
| Agent prose in code | reject low-value comments, temporary reasoning traces, or `AI generated` text |

This removes whole sanity-check flows: review agents, grep probes,
repeated prompt instructions, and inspect-then-fix turns. Tokens go to
the actual change instead of revalidating invariants the repository
already knows. The same contract applies to humans, agents, hooks, and
CI.

See [Agent harness](docs/cli/agent-harness.md) for Codex, Claude Code,
and Gemini CLI hooks.

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

## Documentation

Start with the page that matches the task:

| Task | Page |
| ---- | ---- |
| Inspect symbols from the CLI | [Extract](docs/cli/extract.md) |
| Lint a repository with rules | [Check](docs/cli/check.md) |
| Write rule expressions | [Rule DSL](docs/cli/check-dsl.md) |
| Wire checks into agent hooks or CI | [Agent harness](docs/cli/agent-harness.md) |
| Understand moniker URI syntax | [Moniker URI](docs/design/moniker-uri.md) |
| Build or contribute | [Contributing](CONTRIBUTING.md) |

Full index: [docs/](docs/README.md).

## Performance

The CLI is designed for hooks and CI. Project scans are parallel; per-file
checks are bounded enough for edit hooks. Measurements and reproduction
commands are in [Performance](docs/perf.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE),
at your option. Contributions are accepted under the same terms.
