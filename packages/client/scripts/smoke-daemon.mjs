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
	const syntaxSource =
		"CREATE FUNCTION app.with_default(value text DEFAULT $$fallback$$) " +
		"RETURNS text LANGUAGE plpgsql " +
		"AS $body$ BEGIN RETURN value; END; $body$;";
	const syntax = await client.query({
		op: "syntax_parse",
		language: "sql",
		source: syntaxSource,
		uri: "client-default.sql",
		max_depth: 20,
		max_nodes: 500,
		named_only: true,
		include_text: true,
		max_text_chars: 80,
	});
	if (syntax.result.kind !== "syntax_tree" || syntax.result.data.has_error) {
		throw new Error("the daemon did not parse the dollar-quoted SQL default");
	}
	const syntaxRoot = syntax.result.data.root;
	if (!findSyntaxNode(syntaxRoot, (node) => node.kind === "CreateFunctionStmt")) {
		throw new Error("the syntax tree omitted the function declaration");
	}
	if (
		!findSyntaxNode(
			syntaxRoot,
			(node) =>
				node.kind === "dollar_quoted_string" &&
				node.text === "$$fallback$$",
		)
	) {
		throw new Error("the syntax tree omitted the dollar-quoted default");
	}
	if (
		!findSyntaxNode(
			syntaxRoot,
			(node) => node.kind === "source_file" && node.language === "plpgsql",
		)
	) {
		throw new Error("the syntax tree omitted the PL/pgSQL body");
	}

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
		`daemon smoke passed: syntax.parse and ${symbols.data.total} matching symbol(s), graph focus ${trigger.uri}`,
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

function findSyntaxNode(node, predicate) {
	if (predicate(node)) {
		return node;
	}
	for (const child of node.children) {
		const found = findSyntaxNode(child, predicate);
		if (found) {
			return found;
		}
	}
	return undefined;
}
