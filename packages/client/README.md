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

const graph = await client.graph.identity("sql/schema:public");
client.close();
```

The package validates the daemon protocol during the handshake. Its TypeScript
wire types and `PROTOCOL_VERSION` are generated from
`docs/schema/daemon.schema.json`.

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
				binaryCandidates: ["/usr/local/bin/code-moniker"],
			})
		: undefined;
const daemon = entry ?? owned!.entry;
const client = await runtime.connect(daemon);

client.close();
if (owned) {
	await runtime.stopOwned(owned);
}
```

`stopOwned` verifies both the registered PID and claim token before requesting
shutdown, so a replaced registry claim cannot stop another consumer's daemon.
Stopping a daemon that was not launched by the caller remains the explicit
`runtime.stop(entry)` operation. `runtime.restart(entry, options)` confirms
that the old PID has exited before removing its claim and launching a
replacement.
