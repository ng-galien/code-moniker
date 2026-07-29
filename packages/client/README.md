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

The portable client does not discover, start, stop, or own daemon processes. A
runtime without a standard global `WebSocket` must provide `webSocketFactory`.
