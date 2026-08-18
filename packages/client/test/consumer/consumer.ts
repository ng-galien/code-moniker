import {
	CodeMonikerClient,
	type IdentityGraphResult,
	type QueryPage,
	type WorkspaceSourceSetDto,
} from "@code-moniker/client";
import {
	NodeDaemonRuntime,
	type OwnedDaemon,
} from "@code-moniker/client/node";
import WebSocket from "ws";

declare const client: CodeMonikerClient;

const sourceSet: WorkspaceSourceSetDto = {
	srcset: "database",
	revision: "42",
	documents: [
		{
			uri: "postgres://database/public/schema.sql",
			language: "sql",
			content: "create table account(id bigint);",
		},
	],
};

void client.sources.replace(sourceSet);
void client.workspace.status().then((status) => {
	const refresh = status.timings?.memory_source_refresh;
	if (refresh?.mode === "incremental") {
		return [refresh.modified, refresh.extraction_jobs, refresh.linkage_invocations];
	}
	return [];
});
const graph: Promise<QueryPage<IdentityGraphResult>> =
	client.graph.identity("sql/schema:public");
void graph;
void client.symbols.search({ text: "account" }).then(readFirstPage);
void client.syntax.parse("sql", "SELECT 1", {
	maxDepth: 1_000,
	maxNodes: 20_000,
});

const runtime = new NodeDaemonRuntime();
const owned: OwnedDaemon | undefined = undefined;
void runtime;
void owned;

void CodeMonikerClient.connect("127.0.0.1:3210", {
	acceptAnyWorkspace: true,
	webSocketFactory: createWebSocket,
});
void CodeMonikerClient.connect("127.0.0.1:3210", {
	acceptAnyWorkspace: true,
	webSocketFactory: createStandardWebSocket,
});
// @ts-expect-error Workspace targeting must be explicit.
void CodeMonikerClient.connect("127.0.0.1:3210", {
	webSocketFactory: createWebSocket,
});

function createWebSocket(url: string) {
	return new WebSocket(url);
}

function createStandardWebSocket(url: string) {
	return new globalThis.WebSocket(url);
}

function readFirstPage(page: {
	data: { total: number };
	nextCursor: unknown;
}) {
	return [page.data.total, page.nextCursor];
}
