# Workspace Daemon

The daemon is the resident host for one canonical workspace set. It owns the
live-indexed workspace and exposes a structured query DSL over **JSON-RPC**.
HTTP MCP, TUI, IDE and interactive CLI are thin clients of the same contract.
Stdio MCP is deliberately in-process: the invoking client owns its index and
closing the stdio session cannot leave a detached daemon behind.
The stdio transport starts before its background preload, so MCP initialize is
never gated by a full workspace scan. Until the atomically built snapshot is
ready, data tools return `workspace_loading`; the client can retry without
restarting the server.

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
code-moniker daemon start  [roots...] [--project N] [--cache DIR] [--live-refresh on-demand|auto] [--supervisor-pid PID]
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

`--supervisor-pid` binds the daemon lifetime to another process. Automatic
launchers also pass a private inherited liveness channel: EOF stops the daemon
immediately, even if the operating system has already reused the supervisor
PID. The PID check remains a fallback for manual launchers. Both mechanisms
work during the initial index and remove the daemon's own registry claim on
exit. Every `connect_or_start` launch and the VS Code extension use this mode.
VS Code also requests an explicit shutdown during normal deactivation;
supervision is the crash-safe fallback. Only an explicit foreground `daemon
start` without supervision is persistent by design.

Initial indexing carries a cooperative cancellation token through source
walking, parallel extraction and snapshot build phases. Shutdown cancels that
token before stopping the runtime and never starts a live watcher after
cancellation. Process shutdown is also bounded, so even a source read blocked
inside the operating system cannot keep a supervised daemon or stdio MCP alive.

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

`protocol_version` guards the serialized request/response shape. CLI, MCP, TUI,
and VS Code connect-or-start clients require an exact protocol and
workspace-root match and recycle a protocol-mismatched registered daemon once.
If the replacement still reports another protocol,
the client stops with reinstall guidance instead of entering a restart loop.
It never reuses a daemon that merely contains the requested roots as a subset.
The capability set remains the compatibility signal for individual query
verbs; the daemon package version string is informational.

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
heartbeat, roots, and a state: `indexing` or `ready`. Entries are written
atomically; on exit the daemon removes only its own entry.

Connect-or-start clients purge dead-PID entries, require an exact workspace
identity, validate the daemon handshake roots, and allow up to 30 seconds for a
new daemon to finish its initial index before reporting a readiness timeout. A
ready entry whose endpoint or handshake is unusable is removed with an
ownership check and replaced once; a failed replacement is reported instead of
entering a restart loop.

A live PID with an unavailable endpoint keeps a fresh registry claim and is
reported as an error; clients never unlink it and start a competitor. The
daemon refreshes a registry heartbeat every two seconds and exits if its own
`(pid, token)` claim disappears. A claim with no heartbeat (legacy) or one
older than 15 seconds is expired when its endpoint is unreachable, which also
covers stale JSON whose PID has since been reused by an unrelated process.
Heartbeat replacement and ownership-checked removal share an inter-process
file lock, so a heartbeat cannot recreate a claim concurrently being removed.

VS Code records the exact `(pid, registry token)` claim created by the current
extension host. Only that owner shuts it down on deactivation; PID equality
alone is never treated as ownership. An attached second window reconnects and
starts a new supervised daemon if the owner window exits; it does not remain
bound to a dead socket. A killed extension host closes the inherited
supervision channel, so its daemon exits without relying on PID polling or PID
identity. `--supervisor-pid` remains the compatibility fallback.

Index-creating commands refuse a filesystem root as their workspace. This
prevents a misresolved MCP `cwd` or relative `.` argument from indexing the
whole machine; pass the canonical absolute project directory instead. Identity
resolution remains available to `daemon status` and `daemon stop`, so an old
root daemon can still be diagnosed and removed safely.

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
