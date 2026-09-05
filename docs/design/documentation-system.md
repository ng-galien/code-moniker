# Documentation system

Status: accepted

## Decision

Every public Code Moniker surface follows one documentation vertical:

1. **Discover** the available surface from the running product.
2. **Learn** its mental model and safe workflow progressively.
3. **Use** a task-oriented guide with copyable examples.
4. **Reference** exact grammar, schemas, limits and semantics.

The layers may have different technical owners, but they must link to one
another and identify the same canonical contract. A skill, README or help string
is a router, not a private copy of product documentation.

## Surface map

| Surface | Discover | Learn and use | Exact reference | Canonical contract |
| --- | --- | --- | --- | --- |
| One-shot CLI | `code-moniker --help`, `<command> --help` | Command page under `docs/cli/` | Command page options and exit codes | Clap arguments and command implementation |
| Rules and architecture | `code-moniker rules learn` | Embedded `.cm.md` learn and catalog scenarios | `docs/cli/check-dsl.md` | Rule parser plus executable scenarios |
| Indexed Query DSL | `query.describe` | `docs/cli/query.md` | Live per-verb description and generated daemon schema | Query capability registry and parser |
| MCP for agents | MCP `tools/list` | `docs/cli/mcp.md` and the installed skill | Live tool JSON Schemas and output contract | MCP tool registry and descriptors |
| Workspace daemon | `daemon list`, `daemon status` | `docs/daemon.md` | Generated daemon schema | Typed query protocol and daemon runtime |
| Language vocabulary | `langs`, `shapes` | `rules learn languages` and language recipes | `docs/cli/langs.md` | Registered extractors, kinds and shapes |
| TypeScript client | Package exports and generated types | `packages/client/README.md` | Generated TypeScript types and daemon schema | `@code-moniker/client` source plus generated protocol |
| VS Code extension | Activity bar and command palette | `docs/vscode-extension.md` | Extension settings and package manifest | Extension implementation and packaged CLI |

## Ownership rules

- Dynamic inventories remain dynamic. Query verbs come from `query.describe`,
  MCP fields from `tools/list`, language tags from `langs`, and rule/catalog
  topics from `rules learn`. Narrative pages teach how to use these inventories;
  they do not maintain a second exhaustive registry.
- The embedded skill contains operating decisions and routes agents to product
  discovery. It must not grow private copies of Query, MCP or rule grammar.
- `docs/README.md` is the canonical human navigation tree. The root README may
  expose common entry points but links back to that complete map.
- `code-moniker docs [page]` embeds the Markdown pages under `docs/` and the
  daemon schema for offline reading. `crates/cli/assets/docs` links to this
  canonical tree; Cargo packages the linked content. The embedded inventory
  lives in `crates/cli/src/docs.rs`; coverage tests prevent omitted pages.
  This reader needs neither a workspace nor a daemon. It complements live
  discovery and the executable `rules learn` tutorials.
- `docs/cli/` owns user-facing CLI and MCP workflows. `docs/design/` records
  cross-surface decisions and invariants. Runtime and protocol internals stay in
  their owning component documentation.
- Executable `.cm.md` files are both progressive learning material and tested
  examples. General prose examples should link to them instead of drifting into
  an unvalidated parallel catalog.
- Generated schemas and types are reference artifacts, not introductory
  documentation. Guides link to them after establishing the relevant model.

## Change checklist

When a public command, query verb, MCP tool, language, rule construct, client
method or extension feature changes:

1. update its canonical registry, parser or schema owner;
2. update the live discovery output and its tests;
3. update or add the task guide and exact reference;
4. route the relevant help, documentation index and agent skill to that owner;
5. add or update an executable example when the behavior can be demonstrated;
6. verify local Markdown links and the focused command, schema or scenario
   contract.

This is one documentation change across layers, not permission to restate the
same contract in every layer.
