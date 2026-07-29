import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import process from "node:process";

import { NodeDaemonRuntime } from "../dist/node.js";

const [binaryArgument] = process.argv.slice(2);
if (!binaryArgument) {
	throw new Error(
		"usage: npm run test:daemon:owned -- <code-moniker-binary>",
	);
}

const binary = resolve(binaryArgument);
const workspaceRoot = mkdtempSync(
	join(tmpdir(), "code-moniker-client-consumer-"),
);
const runtime = new NodeDaemonRuntime();
let owned;
let client;

try {
	owned = await runtime.launch({
		workspaceRoots: [workspaceRoot],
		binaryCandidates: [binary],
	});
	client = await runtime.connect(owned.entry, {
		clientName: "@code-moniker/client-owned-smoke",
	});
	await runtime.waitUntilReady(client, { timeoutMs: 60_000 });

	await client.sources.replace({
		srcset: "consumer-sql",
		revision: "1",
		documents: [
			{
				uri: "postgres://consumer/public/schema.sql",
				language: "sql",
				content: `
CREATE TABLE owned_client_account(id BIGINT PRIMARY KEY);
CREATE VIEW owned_client_account_view AS
SELECT id FROM owned_client_account;
`,
			},
		],
	});

	const symbols = await client.symbols.search(
		{
			text: "owned_client_account_view",
			language: ["sql"],
		},
		{ consistency: "stale_ok" },
	);
	let view;
	for (const symbol of symbols.data.rows) {
		if (symbol.name === "owned_client_account_view") {
			view = symbol;
			break;
		}
	}
	if (!view) {
		throw new Error(
			"the owned daemon did not index the external consumer source set",
		);
	}
	const graph = await client.graph.symbol(
		view.uri,
		{},
		{ consistency: "stale_ok" },
	);
	if (
		graph.focus.kind !== "symbol" ||
		graph.focus.symbol.uri !== view.uri
	) {
		throw new Error("the owned daemon did not return the view graph");
	}

	console.log(
		`owned daemon smoke passed: pid ${owned.entry.pid}, graph focus ${view.uri}`,
	);
} finally {
	client?.close();
	if (owned) {
		await runtime.stopOwned(owned, {
			exitTimeoutMs: 10_000,
		});
	}
	rmSync(workspaceRoot, { recursive: true, force: true });
}
