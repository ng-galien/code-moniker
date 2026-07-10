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

`daemon start` runs in the foreground; clients auto-spawn a background daemon via
`connect_or_start`. `query` field syntax is positional for the URI, e.g.
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

`workspace.status`, `tree.children`, `symbol.search`, `symbol.insights`,
`symbol.detail`, `symbol.usages`, `symbol.graph`, `identity.children`,
`view.read`, `rules.list`, `rules.check`, `change.review`, `notes`. Command
verbs: `workspace.refresh`.

`symbol.graph focus:"<symbol URI or rel path>"` returns the ego-centric
neighborhood of a unit: the focus defines a boundary on the identity tree,
and resolved references partition into internal edges, callers (outside-in)
and callees (inside-out), aggregated per neighbor with relation kinds and
call counts; unresolved references are counted, never dropped. This feeds
the IDE Graph Explorer triptych and the `code_moniker_graph` MCP tool.

`identity.children prefix:"<identity prefix>"` returns one level of the
identity tree - the purely symbolic navigation surface, no filesystem. Each
child segment carries its kind/name (`package:acme`, `module:pairing`,
`fn:pair_file(...)`), aggregate def counts, and the full `SymbolDto` when the
segment itself is a navigable definition. An empty prefix lists the roots
(`srcset:*`, `lang:*`); full moniker URIs are accepted and normalized.

## Discovery

A registry directory under `$TMPDIR/code-moniker-daemons/` holds one `<hash>.json`
per workspace config (hash of roots/project/cache/live-refresh). Each entry
records `endpoint` (`127.0.0.1:port`), `token`, `pid`, and the roots. Clients read
the entry for their config hash and connect to the endpoint. On exit the daemon
removes its registry file.

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
