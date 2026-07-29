import process from "node:process";

import WebSocket from "ws";

import { CodeMonikerClient } from "../dist/index.js";

const [endpoint, workspaceRoot] = process.argv.slice(2);
if (!endpoint || !workspaceRoot) {
	throw new Error(
		"usage: npm run test:daemon -- <endpoint> <canonical-workspace-root>",
	);
}

const srcset = `client-smoke-${process.pid}`;
const client = await CodeMonikerClient.connect(endpoint, {
	clientName: "@code-moniker/client-smoke",
	expectedWorkspaceRoots: [workspaceRoot],
	webSocketFactory: createWebSocket,
});

try {
	await client.sources.replace({
		srcset,
		revision: "1",
		documents: [
			{
				uri: "postgres://client-smoke/public/schema.sql",
				language: "sql",
				content: `
CREATE TABLE client_smoke_account(id BIGINT PRIMARY KEY);
CREATE FUNCTION client_smoke_audit() RETURNS trigger
LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$;
CREATE TRIGGER client_smoke_account_audit
AFTER INSERT ON client_smoke_account
FOR EACH ROW EXECUTE FUNCTION client_smoke_audit();
`,
			},
		],
	});

	const symbols = await client.symbols.search({
		text: "client_smoke_account_audit",
		language: ["sql"],
	}, {
		consistency: "stale_ok",
	});
	let trigger;
	for (const symbol of symbols.data.rows) {
		if (symbol.name === "client_smoke_account_audit") {
			trigger = symbol;
			break;
		}
	}
	if (!trigger) {
		throw new Error("the daemon did not index the published SQL trigger");
	}

	const graph = await client.graph.symbol(trigger.uri, {}, {
		consistency: "stale_ok",
	});
	if (graph.focus.kind !== "symbol" || graph.focus.symbol.uri !== trigger.uri) {
		throw new Error("the daemon did not return the trigger graph");
	}

	console.log(
		`daemon smoke passed: ${symbols.data.total} matching symbol(s), graph focus ${trigger.uri}`,
	);
} finally {
	try {
		await client.sources.remove(srcset);
	} finally {
		client.close();
	}
}

function createWebSocket(url) {
	return new WebSocket(url);
}
