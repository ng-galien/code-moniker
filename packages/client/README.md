# `@code-moniker/client`

Typed JavaScript and TypeScript client for an already running Code Moniker
workspace daemon.

```ts
import { CodeMonikerClient } from "@code-moniker/client";

const client = await CodeMonikerClient.connect("127.0.0.1:3210", {
	expectedWorkspaceRoots: ["/workspace/project"],
	webSocketFactory: (url) => new WebSocket(url),
});

await client.sources.replace({
	srcset: "database",
	revision: "42",
	documents: [
		{
			uri: "postgres://database/public/schema.sql",
			language: "sql",
			content: "create table account(id bigint primary key);",
		},
	],
});

const graph = await client.graph.identity("sql/schema:public", {
	path: ["postgres://database/public/**"],
	minCount: 2,
});
console.log(graph.data.coverage, graph.nextCursor);
client.close();
```

The package validates the daemon protocol during the handshake. Its TypeScript
wire types and `PROTOCOL_VERSION` are generated from
`docs/schema/daemon.schema.json`. Every build, type-check and test regenerates
them first; CI then rejects any generated diff, so a protocol change cannot be
merged with a stale client.

Connection readiness and workspace readiness are separate. A successful
connection means the endpoint is serving and the handshake is valid; it does
not imply that the initial index is complete. Inspect `client.workspace.status()`
for the typed `loading | ready | refreshing | failed` phase. Data calls made
during initial loading reject immediately with `DaemonRpcError.code ===
"workspace_loading"`; the library does not hide index duration behind a polling
timeout. Subscribe to `refreshed` or `failed`, or retry according to the
consumer's own UX policy.

`client.graph.identity()` returns a `QueryPage<IdentityGraphResult>` because
identity graphs are generation-aware and paginated. Pass `path` to scope files
before identity aggregation and `minCount` to filter weak edges while retaining
their pre-filter totals in `coverage`. `client.symbols.usages()` keeps exact
symbol semantics by default; `includeDescendants: true` explicitly rolls
navigable member activity into an owner and removes internal relations.

Workspace targeting is explicit: pass `expectedWorkspaceRoots` to validate the
daemon identity, or pass `acceptAnyWorkspace: true` when the caller deliberately
accepts the endpoint's workspace. Omitting both is a TypeScript error.

Queries default to the fail-closed `current` consistency. A consumer that
deliberately wants the daemon's pinned indexed snapshot can pass
`{ consistency: "stale_ok" }`; a consumer that wants the filesystem refreshed
first can pass `{ consistency: "refresh_if_stale" }`.

The generated query union includes bounded `syntax_tree` and stateless
`syntax_parse` operations. See
[On-demand syntax tree](../../docs/cli/mcp-syntax-tree.md) for its TypeScript
request, result, limits, and error contract. Nodes at the root of an embedded
language tree expose the optional `language` field, for example `plpgsql`.

The portable entry point does not discover, start, stop, or own daemon
processes. A runtime without a standard global `WebSocket` must provide
`webSocketFactory`.

Node.js consumers can opt into those responsibilities through the dedicated
subpath:

```ts
import { NodeDaemonRuntime } from "@code-moniker/client/node";

const runtime = new NodeDaemonRuntime();
const entry = runtime.findDaemon(["/workspace/project"]);
const owned =
	entry === undefined
		? await runtime.launch({
				workspaceRoots: ["/workspace/project"],
			})
		: undefined;
const daemon = entry ?? owned!.entry;
const client = await runtime.connect(daemon);

client.close();
if (owned) {
	await runtime.stopOwned(owned);
}
```

The Node entry point resolves the matching precompiled Code Moniker binary from
the package's platform-specific optional dependency on macOS, Linux, and
Windows x64. The Linux package contains the statically linked musl release
binary, so the same npm package works on glibc distributions and Alpine without
a libc-specific install script. An explicit `binaryCandidates` list on the
runtime or an individual launch remains available for development builds and
custom installations. If optional dependencies were omitted during
installation, the runtime falls back to `code-moniker` on `PATH` and reports
every attempted candidate on failure.

`stopOwned` verifies both the registered PID and claim token before requesting
shutdown, so a replaced registry claim cannot stop another consumer's daemon.
Stopping a daemon that was not launched by the caller remains the explicit
`runtime.stop(entry)` operation. `runtime.restart(entry, options)` confirms
that the old PID has exited before removing its claim and launching a
replacement.
