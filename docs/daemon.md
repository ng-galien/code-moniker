# Workspace Daemon

The daemon is the resident host for one canonical workspace set. It owns the
live-indexed workspace and exposes a structured query DSL over **JSON-RPC**.
Every product surface — MCP, TUI, IDE, interactive CLI — is a thin client of the
same contract; none of them owns the workspace.

## Crate layout

| crate | role |
|---|---|
| `code-moniker-query` | pure DTOs + query DSL parse/format + daemon discovery; optional `rpc` feature exposes the jsonrpsee `#[rpc(server, client)]` contract |
| `code-moniker-daemon` | resident server: owns the workspace, implements the RPC server |
| `code-moniker-daemon-client` | reusable sync client (jsonrpsee WS client behind a dedicated runtime) |

jsonrpsee is the transport only and is feature-gated on `code-moniker-query`.
The query DSL (`QueryRequest`/`QueryResponse`) travels as opaque method
params/results.

## Commands

```
code-moniker daemon start  [roots...] [--project N] [--cache DIR] [--live-refresh on-demand|auto]
code-moniker daemon status [roots...]
code-moniker daemon stop   [roots...]
code-moniker daemon list
code-moniker query [-r root] "<DSL>" [--json]
```

`daemon start` runs in the foreground and does not report `index ready` until
the workspace is registered and available for queries. Clients auto-spawn a
background daemon via `connect_or_start`; concurrent clients share its atomic
registry claim rather than creating competing processes. `query` field syntax is positional for the URI, e.g.
`code-moniker query "view.read workspace/views"`.

## Transport: JSON-RPC over loopback WebSocket

- The daemon binds `127.0.0.1:0` (kernel-assigned port) and serves jsonrpsee WS.
- Multiple clients connect concurrently to one daemon (MCP + TUI + IDE at once).
- Methods (namespace `moniker_`):
  - `handshake(client) -> HandshakeResponse` (protocol version + capabilities)
  - `query(QueryRequest) -> QueryResponse`
  - `command(CommandRequest) -> CommandResponse`
  - `shutdown()`
  - `subscribeEvents` / `events` / `unsubscribeEvents` — subscription stream of
    `WorkspaceEventDto` (stale / refreshed / notes / git-base).

### Query verbs (DSL)

`query.describe`, `workspace.status`, `tree.children`, `symbol.search`,
`symbol.insights`, `symbol.detail`, `symbol.usages`, `symbol.graph`,
`identity.children`, `identity.graph`, `view.read`, `rules.list`,
`rules.check`, `rules.applicable`, `change.review`, `change.context`,
`resolution.audit`, `notes`. Command verbs: `workspace.refresh`.

`query.describe [verb:"..."]` is generated from the canonical capability
registry. It reports fields, defaults, required values, pagination and
projectable result fields. MCP agents normally reach this through the
read-only `code_moniker_query` escape hatch; direct daemon queries remain a
developer and protocol-diagnostic surface.

`change.context focus:"<symbol URI or rel path>" max_items:20` returns a
bounded pre-change view: graph neighborhood and resolution coverage, active
notes, applicable rules, existing worktree changes and canonical suggested
checks. The specialized MCP entry is `code_moniker_context`.

`rules.applicable focus:"..."` explains whether a compiled rule is
applicable, ignored or only potential for the selected symbol/file scope.

`symbol.graph focus:"<symbol URI or rel path>"` returns the ego-centric
neighborhood of a unit: the focus defines a boundary on the identity tree,
and resolved references partition into internal edges, callers (outside-in)
and callees (inside-out), aggregated per neighbor with relation kinds and
call counts. References without an in-workspace target are decomposed in
`unlinked` (`external`, `manifest_blocked`, `unresolved` with a by-reason
ventilation) so external-by-design links never read as resolution gaps. This
feeds the IDE Graph Explorer and the `code_moniker_graph` MCP tool.
`direction:incoming|outgoing|both`, repeatable `relation:`, `min_count:` and
`include_internal:` apply the same bounded relational filters in the DSL and
MCP surface.

`identity.children prefix:"<identity prefix>"` returns one level of the
identity tree - the purely symbolic navigation surface, no filesystem. Each
child segment carries its kind/name (`package:acme`, `module:pairing`,
`fn:pair_file(...)`), aggregate def counts, and the full `SymbolDto` when the
segment itself is a navigable definition. An empty prefix lists the roots
(`srcset:*`, `lang:*`); full moniker URIs are accepted and normalized.

`identity.graph prefix:"<identity prefix>"` projects that level as a graph:
nodes are the prefix's children, edges are resolved references rolled up to
the pair of child segments they connect (kinds + counts), and boundary
crossings aggregate into `ports_in`/`ports_out` at the scope's own depth.
References from inside the scope without an in-workspace target are
decomposed in `unlinked` (external / manifest_blocked / unresolved by
reason). This feeds the scoped exploration canvas of the IDE Graph Explorer.

## Discovery

A registry directory under `$TMPDIR/code-moniker-daemons/` holds one `<hash>.json`
per workspace identity (roots/project/cache; refresh policy does not create a
second daemon). Each entry records `endpoint` (`127.0.0.1:port`), `token`, `pid`,
roots, and a state: `indexing` or `ready`. Entries are written atomically; on
exit the daemon removes only its own entry.

`daemon status` distinguishes a daemon that is `indexing`, a `ready` daemon, a
live PID with an unreachable endpoint (`stale registry`), and a dead PID (whose
registry entry is removed). `daemon list` also purges dead-PID entries. A status
for a workspace reports any concurrent daemon rooted at an ancestor or child
directory, such as `/trust` and `/trust/apps/trust`.

## Live refresh

`--live-refresh` sets how the daemon reacts to file changes detected by the
FSEvents watcher (`notify::RecommendedWatcher`, shared with the TUI):

- `on-demand` (default): mark the workspace stale; re-extract lazily on the next
  query.
- `auto`: apply the refresh immediately in the background.

Either way the daemon broadcasts a `WorkspaceEventDto` to subscribed clients.

## Security

The daemon listens on loopback only. A per-daemon `token` is generated and stored
in the registry entry; clients read it from the registry. Token enforcement on the
WS handshake is the remaining hardening step (the token is plumbed end-to-end but
not yet validated server-side).
