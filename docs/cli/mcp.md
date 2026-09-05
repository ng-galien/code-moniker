# Code Moniker MCP — indexed agent tools

The MCP server exposes a live Code Moniker workspace index as intent-oriented
tools for agents. Tool names and input schemas are published through MCP `tools/list`;
those live descriptors are the source of truth for exact arguments. This page
explains how the tools fit together and points to the deeper references without
copying every schema field.

## Start the server

For a client-owned local session, use stdio with an absolute project root:

```sh
code-moniker mcp /absolute/path/to/project --transport stdio
```

The agent installer writes the appropriate project configuration:

```sh
code-moniker agent install --client codex
code-moniker agent doctor --client codex
```

HTTP mode binds a loopback endpoint and is intended for explicit integrations
and diagnostics:

```sh
code-moniker mcp /absolute/path/to/project --transport http --port 3210
```

See [Agent integration, hooks, and CI](agent.md) for ownership and installation,
and [Workspace Daemon](../daemon.md) for index lifecycle and freshness.

## Discover the live surface

An MCP client discovers the current tool names, descriptions and JSON Schemas
through `tools/list`. Agents should use those descriptors instead of recalling
field names from prose. The generic `code_moniker_query` tool additionally
exposes the daemon's live Query DSL registry:

```text
code_moniker_query query:'query.describe'
code_moniker_query query:'query.describe verb:"identity.graph"'
```

The first call lists query operations and their preferred intent tools. The
second describes one operation's fields, types, defaults, bounds, pagination,
projection and example. See [Indexed Query DSL](query.md) for the language.

## Choose one intent tool

| Need | Tool | Contract |
| --- | --- | --- |
| Enter the workspace, expand its explorer, read a symbol, or request AST | `code_moniker_read` | Default indexed entry point; the first workspace read verifies `expected_roots` |
| List exact symbols or summarize indexed symbol populations | `code_moniker_symbols` | Exact structural filters and generation-aware pagination |
| Fuzzy symbol lookup | `code_moniker_search` | Search text plus the same structural scope filters |
| Read incoming or outgoing references | `code_moniker_usages` | Exact symbol by default; descendant roll-up is explicit |
| Inspect a file or symbol neighborhood | `code_moniker_graph` | Internal edges, callers, callees, unlinked facts and coverage |
| Gather pre-change evidence | `code_moniker_context` | One snapshot-consistent bundle of graph, notes, rules, changes and suggested checks |
| Inspect or execute project rules | `code_moniker_rules` | `list` for testimony and rationale; `run` for indexed evaluation |
| Read worktree changes as symbol facts | `code_moniker_diff` | `HEAD..worktree`, facts only, no importance judgment |
| Read or maintain symbol notes | `code_moniker_notes` | The dedicated mutating surface for notes and controlled transitions |
| Run an advanced read-only daemon operation | `code_moniker_query` | Use only when no intent tool covers the operation; start with `query.describe` |
| Publish pending filesystem changes | `code_moniker_refresh` | Use after a stale-index response; no arguments |

Do not replay one investigation through several equivalent tools. Start with
the narrowest intent, reuse returned monikers and executable continuation calls,
and broaden only when omitted evidence can change the answer.

## Workspace identity and freshness

The first workspace-wide read must pass the current absolute roots through
`expected_roots`. A mismatch is an error; the server never silently changes the
workspace behind an established session.

Stdio MCP owns an in-process worker whose initial index may still be loading
after the MCP transport initializes. `workspace.status` exposes
`loading`, `ready`, `refreshing`, or `failed`. Data tools return the typed
`workspace_loading` error instead of hiding the scan behind a long request.

With on-demand refresh, a filesystem change marks the published generation
stale. Use `code_moniker_refresh` when current filesystem state is required.
An existing generation and its cursors remain immutable while the next one is
built.

## Output and bounds

Agent tools share one presentation contract:

- text is the default and renders compact Markdown;
- `compact:true` is the default for reusable short monikers;
- `budget:"small"` is the default structural volume profile;
- `format:"json"` returns the complete typed projection as MCP
  `structuredContent` and ignores text presentation options;
- paginated responses return an executable next call that preserves generation
  and presentation mode.

Budgets are applied to typed results before rendering. Output is never sliced
at an arbitrary character boundary. Request `medium`, `full`, source code, or a
later page only when the current bounded response shows that it is needed. The
normative representation decision is recorded in
[Agent output boundary](../design/agent-output-boundary.md).

## AST and project rules

`code_moniker_read` has two syntax modes:

- `uri:<file-or-moniker> ast:true` reads a bounded tree from indexed source;
- `language:<tag> source:<text>` parses direct text without loading or mutating
  a workspace index.

See [On-demand syntax tree](mcp-syntax-tree.md) for exact AST fields, limits,
responses and errors.

Project rule authoring is a different surface. Learn it from the embedded
Markdown corpus with `code-moniker rules learn`, use
[Check](check.md) for execution, and use the [Rule DSL](check-dsl.md) for the
grammar. The `ast` rule domain reuses that rule language; it is not an MCP query
language.

## Authoritative artifacts

- Tool registry and shared output contract: `crates/cli/src/mcp/tools/mod.rs`
- Per-tool descriptions and schemas: `crates/cli/src/mcp/tools/`
- Agent-facing Markdown templates: `crates/cli/templates/`
- Query capability registry: `crates/query/src/lib.rs`
- Installed skill router: `agents/skills/code-moniker/`
