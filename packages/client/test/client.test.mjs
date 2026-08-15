import assert from "node:assert/strict";
import test from "node:test";

import {
	CodeMonikerClient,
	DaemonRpcError,
	PROTOCOL_VERSION,
	ProtocolMismatchError,
	WorkspaceMismatchError,
	WorkspaceTargetRequiredError,
} from "../dist/index.js";

const ROOT = "/workspace/project";

test("connect validates the handshake and exposes daemon capabilities", async () => {
	const daemon = new FakeDaemon();
	const client = await CodeMonikerClient.connect("127.0.0.1:3210", {
		clientName: "consumer-test",
		expectedWorkspaceRoots: [ROOT],
		webSocketFactory: daemon.factory,
	});

	assert.equal(daemon.url, "ws://127.0.0.1:3210");
	assert.equal(daemon.requests[0].method, "moniker_handshake");
	assert.deepEqual(daemon.requests[0].params, ["consumer-test"]);
	assert.equal(client.handshake.protocol_version, PROTOCOL_VERSION);
	assert.equal(client.supportsQuery("symbol.search"), true);
	assert.equal(client.supportsCommand("workspace.source_set.replace"), true);
	assert.equal(client.supportsEvent("refreshed"), true);
	client.close();
});

test("source-set operations send typed atomic commands", async () => {
	const daemon = new FakeDaemon();
	const client = await connect(daemon);
	const sourceSet = {
		srcset: "postgres",
		revision: "tx:42",
		documents: [
			{
				uri: "postgres://database/public/schema.sql",
				language: "sql",
				content: "create table account(id bigint primary key);",
			},
		],
	};

	const replace = await client.sources.replace(sourceSet);
	const remove = await client.sources.remove("postgres");
	await client.workspace.refresh();

	assert.equal(replace.message, "source set replaced");
	assert.equal(remove.message, "source set removed");
	assert.deepEqual(commandRequests(daemon), [
		{ op: "workspace_source_set_replace", source_set: sourceSet },
		{ op: "workspace_source_set_remove", srcset: "postgres" },
		{ op: "workspace_refresh" },
	]);
	client.close();
});

test("the diff-impact facade sends both virtual revisions in one transactional query", async () => {
	const daemon = new FakeDaemon();
	const client = await connect(daemon);
	const sourceSet = (revision, content) => ({
		srcset: "diff-impact",
		revision,
		documents: [{ uri: "src/lib.rs", language: "rs", content }],
	});
	const result = await client.diffImpact.compare({
		scope: "base..head",
		project: "sample",
		base: sourceSet("base", "pub fn changed() { old(); }"),
		head: sourceSet("head", "pub fn changed() { new(); }"),
		files: [{
			status: "modified",
			old_uri: "src/lib.rs",
			new_uri: "src/lib.rs",
			old_hunks: [{ start: 1, end: 1 }],
			new_hunks: [{ start: 1, end: 1 }],
			rename_score: null,
		}],
	});

	assert.equal(result.scope, "base..head");
	assert.equal(queryRequests(daemon).at(-1).op, "diff_impact_compare");
	assert.equal(queryRequests(daemon).at(-1).base.revision, "base");
	assert.equal(queryRequests(daemon).at(-1).head.revision, "head");
	client.close();
});

test("symbol and graph facades map ergonomic options to the public query protocol", async () => {
	const daemon = new FakeDaemon();
	const client = await connect(daemon);

	const symbols = await client.symbols.search({ text: "account", language: ["sql"] });
	const usages = await client.symbols.usages("code+moniker://account", {
		direction: "both",
		includeDescendants: true,
	});
	const graph = await client.graph.identity("sql/schema:public", {
		path: ["src/main/**"],
		minCount: 2,
	});

	assert.equal(symbols.data.total, 1);
	assert.equal(usages.data.target.uri, "code+moniker://account");
	assert.equal(graph.data.prefix, "sql/schema:public");
	assert.deepEqual(queryRequests(daemon), [
		{
			op: "symbol_search",
			workspace: null,
			text: "account",
			path: [],
			lang: ["sql"],
			kind: [],
			shape: [],
			name: null,
			include_non_navigable: false,
			include_code: false,
			context_lines: 0,
			projection: [],
		},
		{
			op: "symbol_usages",
			workspace: null,
			uri: "code+moniker://account",
			direction: "both",
			path: [],
			lang: [],
			include_descendants: true,
			projection: [],
		},
		{
			op: "identity_graph",
			workspace: null,
			prefix: "sql/schema:public",
			path: ["src/main/**"],
			min_count: 2,
		},
	]);
	client.close();
});

test("syntax facades forward client-selected parse and tree budgets without clamping", async () => {
	const daemon = new FakeDaemon();
	const client = await connect(daemon);

	await client.syntax.parse("sql", "SELECT address_1.id FROM shop.address AS address_1;", {
		uri: "selection.sql",
		maxDepth: 1_000,
		maxNodes: 20_000,
	});
	await client.syntax.tree("src/query.sql", {
		workspace: ROOT,
		maxDepth: 64,
		maxNodes: 2_000,
		namedOnly: false,
		includeText: true,
		maxTextChars: 120,
	});

	assert.deepEqual(queryRequests(daemon).slice(-2), [
		{
			op: "syntax_parse",
			language: "sql",
			source: "SELECT address_1.id FROM shop.address AS address_1;",
			uri: "selection.sql",
			max_depth: 1_000,
			max_nodes: 20_000,
			named_only: true,
			include_text: false,
			max_text_chars: 80,
		},
		{
			op: "syntax_tree",
			workspace: ROOT,
			focus: "src/query.sql",
			max_depth: 64,
			max_nodes: 2_000,
			named_only: false,
			include_text: true,
			max_text_chars: 120,
		},
	]);
	client.close();
});

test("paginated facades preserve the cursor and generation across pages", async () => {
	const daemon = new FakeDaemon();
	const client = await connect(daemon);

	const first = await client.symbols.search(
		{ text: "account" },
		{ limit: 1 },
	);
	assert.deepEqual(first.nextCursor, { offset: 1, generation: 8 });
	assert.equal(first.generation, 8);

	const second = await client.symbols.search(
		{ text: "account" },
		{ limit: 1, cursor: first.nextCursor },
	);
	assert.equal(second.nextCursor, null);
	assert.deepEqual(
		daemon.requests.at(-1).params[0].page.cursor,
		first.nextCursor,
	);
	client.close();
});

test("workspace events are delivered through disposable subscriptions", async () => {
	const daemon = new FakeDaemon();
	const client = await connect(daemon);
	const events = [];
	const subscription = await client.events.subscribe((event) => events.push(event));

	daemon.emitSubscription("events-1", {
		kind: "refreshed",
		generation: 9,
	});
	await Promise.resolve();
	subscription.dispose();

	assert.deepEqual(events, [{ kind: "refreshed", generation: 9 }]);
	assert.equal(daemon.requests.at(-1).method, "moniker_unsubscribeEvents");
	client.close();
});

test("protocol and workspace mismatches fail closed", async () => {
	const stale = new FakeDaemon({ protocolVersion: PROTOCOL_VERSION - 1 });
	await assert.rejects(
		() => connect(stale),
		(error) =>
			error instanceof ProtocolMismatchError &&
			error.expected === PROTOCOL_VERSION &&
			error.actual === PROTOCOL_VERSION - 1 &&
			error.direction === "daemon_older",
	);

	const newer = new FakeDaemon({ protocolVersion: PROTOCOL_VERSION + 1 });
	await assert.rejects(
		() => connect(newer),
		(error) =>
			error instanceof ProtocolMismatchError &&
			error.expected === PROTOCOL_VERSION &&
			error.actual === PROTOCOL_VERSION + 1 &&
			error.direction === "daemon_newer",
	);

	const wrongWorkspace = new FakeDaemon({ workspaceRoots: ["/another/project"] });
	await assert.rejects(
		() => connect(wrongWorkspace),
		(error) =>
			error instanceof WorkspaceMismatchError &&
			error.expected.includes(ROOT) &&
			error.actual.includes("/another/project"),
	);

	const duplicateRoots = new FakeDaemon({ workspaceRoots: [ROOT, ROOT] });
	await assert.rejects(
		() =>
			CodeMonikerClient.connect("127.0.0.1:3210", {
				expectedWorkspaceRoots: [ROOT, "/second/root"],
				webSocketFactory: duplicateRoots.factory,
			}),
		WorkspaceMismatchError,
	);
});

test("JavaScript callers must choose a workspace target before any socket opens", async () => {
	await assert.rejects(
		() => CodeMonikerClient.connect("127.0.0.1:3210"),
		WorkspaceTargetRequiredError,
	);

	for (const target of [{}, { expectedWorkspaceRoots: [] }]) {
		const daemon = new FakeDaemon();
		await assert.rejects(
			() =>
				CodeMonikerClient.connect("127.0.0.1:3210", {
					...target,
					webSocketFactory: daemon.factory,
				}),
			WorkspaceTargetRequiredError,
		);
		assert.equal(daemon.url, undefined);
	}

	const conflicting = new FakeDaemon();
	await assert.rejects(
		() =>
			CodeMonikerClient.connect("127.0.0.1:3210", {
				expectedWorkspaceRoots: [ROOT],
				acceptAnyWorkspace: true,
				webSocketFactory: conflicting.factory,
			}),
		WorkspaceTargetRequiredError,
	);
	assert.equal(conflicting.url, undefined);
});

test("structured daemon errors preserve their machine-readable code", async () => {
	const daemon = new FakeDaemon({
		errors: {
			moniker_query: {
				message: "workspace is stale",
				code: "workspace_stale",
			},
		},
	});
	const client = await connect(daemon);

	await assert.rejects(
		() => client.workspace.status(),
		(error) =>
			error instanceof DaemonRpcError &&
			error.code === "workspace_stale" &&
			error.message === "workspace is stale",
	);
	client.close();
});

test("calls time out and closed connections reject new work", async () => {
	const silent = new FakeDaemon({ silentMethods: ["moniker_query"] });
	const client = await CodeMonikerClient.connect("127.0.0.1:3210", {
		expectedWorkspaceRoots: [ROOT],
		timeoutMs: 5,
		webSocketFactory: silent.factory,
	});

	await assert.rejects(
		() => client.workspace.status(),
		/daemon call moniker_query timed out/,
	);
	silent.socket.emit("close", {});
	await assert.rejects(
		() => client.workspace.status(),
		/daemon connection is closed/,
	);
});

function connect(daemon) {
	return CodeMonikerClient.connect("ws://127.0.0.1:3210", {
		expectedWorkspaceRoots: [ROOT],
		webSocketFactory: daemon.factory,
	});
}

function commandRequests(daemon) {
	const commands = [];
	for (const request of daemon.requests) {
		if (request.method === "moniker_command") {
			commands.push(request.params[0].command);
		}
	}
	return commands;
}

function queryRequests(daemon) {
	const queries = [];
	for (const request of daemon.requests) {
		if (request.method === "moniker_query") {
			queries.push(request.params[0].query);
		}
	}
	return queries;
}

class FakeDaemon {
	constructor(options = {}) {
		this.protocolVersion = options.protocolVersion ?? PROTOCOL_VERSION;
		this.workspaceRoots = options.workspaceRoots ?? [ROOT];
		this.errors = options.errors ?? {};
		this.silentMethods = new Set(options.silentMethods ?? []);
		this.requests = [];
		this.url = undefined;
		this.socket = undefined;
		this.factory = this.createSocket.bind(this);
	}

	createSocket(url) {
		this.url = url;
		this.socket = new FakeWebSocket(this);
		return this.socket;
	}

	receive(payload) {
		const request = JSON.parse(payload);
		this.requests.push(request);
		if (this.silentMethods.has(request.method)) {
			return;
		}
		const configuredError = this.errors[request.method];
		if (configuredError) {
			this.socket.emit("message", {
				data: JSON.stringify({
					jsonrpc: "2.0",
					id: request.id,
					error: {
						message: configuredError.message,
						data: configuredError,
					},
				}),
			});
			return;
		}
		let result;
		switch (request.method) {
			case "moniker_handshake":
				result = {
					protocol_version: this.protocolVersion,
					daemon_version: "0.6.0",
					build: { version: "0.6.0", fingerprint: "test" },
					workspace_root: this.workspaceRoots[0],
					workspace_roots: this.workspaceRoots,
					capabilities: {
						queries: ["symbol.search", "symbol.usages", "identity.graph", "diff-impact.compare"],
						query_mcp_tools: {},
						commands: [
							"workspace.source_set.replace",
							"workspace.source_set.remove",
							"workspace.refresh",
						],
						events: ["refreshed"],
					},
				};
				break;
			case "moniker_command":
				result = {
					message: commandMessage(request.params[0].command.op),
					generation: 8,
					status: null,
				};
				break;
			case "moniker_query":
				result = queryResponse(
					request.params[0].query,
					request.params[0].page,
				);
				break;
			case "moniker_subscribeEvents":
				result = "events-1";
				break;
			case "moniker_unsubscribeEvents":
				result = null;
				break;
			default:
				throw new Error(`unexpected method ${request.method}`);
		}
		this.socket.emit("message", {
			data: JSON.stringify({ jsonrpc: "2.0", id: request.id, result }),
		});
	}

	emitSubscription(subscription, result) {
		this.socket.emit("message", {
			data: JSON.stringify({
				jsonrpc: "2.0",
				method: "moniker_events",
				params: { subscription, result },
			}),
		});
	}
}

class FakeWebSocket {
	constructor(daemon) {
		this.daemon = daemon;
		this.listeners = new Map();
	}

	addEventListener(type, listener) {
		const listeners = this.listeners.get(type) ?? [];
		listeners.push(listener);
		this.listeners.set(type, listeners);
		if (type === "open") {
			listener({});
		}
	}

	removeEventListener(type, listener) {
		const listeners = this.listeners.get(type) ?? [];
		const retained = [];
		for (const candidate of listeners) {
			if (candidate !== listener) {
				retained.push(candidate);
			}
		}
		this.listeners.set(type, retained);
	}

	send(payload) {
		this.daemon.receive(payload);
	}

	close() {
		this.emit("close", {});
	}

	emit(type, event) {
		for (const listener of this.listeners.get(type) ?? []) {
			listener(event);
		}
	}
}

function commandMessage(op) {
	if (op === "workspace_source_set_replace") {
		return "source set replaced";
	}
	if (op === "workspace_source_set_remove") {
		return "source set removed";
	}
	return "workspace refreshed";
}

function queryResponse(query, page) {
	if (query.op === "syntax_parse" || query.op === "syntax_tree") {
		return {
			generation: query.op === "syntax_tree" ? 8 : null,
			next_cursor: null,
			result: {
				kind: "syntax_tree",
				data: {
					file: query.uri ?? query.focus,
					language: query.language ?? "sql",
					focus: query.uri ?? query.focus,
					focus_line_range: null,
					root: {
						kind: "source_file",
						named: true,
						error: false,
						missing: false,
						byte_range: [0, 0],
						start: { line: 1, column: 0 },
						end: { line: 1, column: 0 },
						children: [],
					},
					emitted_nodes: 1,
					total_nodes: 1,
					max_depth: query.max_depth,
					truncated: false,
					has_error: false,
				},
			},
		};
	}
	if (query.op === "diff_impact_compare") {
		return {
			generation: null,
			next_cursor: null,
			result: {
				kind: "diff_impact",
				data: {
					scope: query.scope,
					summary: { files: 1, analyzable_files: 1, symbol_changes: 1, ref_changes: 0, retargeted_refs: 0, residual_files: 0 },
					files: [], symbol_changes: [], ref_changes: [], diagnostics: [],
				},
			},
		};
	}
	if (query.op === "symbol_search") {
		return {
			generation: 8,
			next_cursor:
				page.cursor === null
					? { offset: 1, generation: 8 }
					: null,
			result: {
				kind: "symbol_list",
				data: {
					total: 1,
					rows: [
						{
							file: "postgres://database/public/schema.sql",
							id: "account",
							kind: "table",
							language: "sql",
							name: "account",
							navigable: true,
							root: ROOT,
							signature: "account",
							uri: "code+moniker://account",
							visibility: "public",
						},
					],
				},
			},
		};
	}
	if (query.op === "symbol_usages") {
		return {
			generation: 8,
			next_cursor: null,
			result: {
				kind: "symbol_usages",
				data: {
					target: {
						file: "postgres://database/public/schema.sql",
						id: "account",
						kind: "table",
						language: "sql",
						name: "account",
						navigable: true,
						root: ROOT,
						signature: "account",
						uri: "code+moniker://account",
						visibility: "public",
					},
					direction: query.direction,
					include_descendants: query.include_descendants,
					targets: query.include_descendants ? 2 : 1,
					rows: [],
					total: 0,
				},
			},
		};
	}
	return {
		generation: 8,
		next_cursor: null,
		result: {
			kind: "identity_graph",
			data: {
				prefix: query.prefix,
				path: query.path,
				min_count: query.min_count,
				coverage: {
					rows_total: 0,
					rows_matching: 0,
					rows_emitted: 0,
					nodes_total: 0,
					nodes_emitted: 0,
					edges_total: 0,
					edges_matching: 0,
					edges_emitted: 0,
					ports_in_total: 0,
					ports_in_matching: 0,
					ports_in_emitted: 0,
					ports_out_total: 0,
					ports_out_matching: 0,
					ports_out_emitted: 0,
				},
				nodes: [],
				edges: [],
				ports_in: [],
				ports_out: [],
				unlinked: {
					candidate: 0,
					dependency: 0,
					dynamic: 0,
					external: 0,
					injected_external: 0,
					manifest_blocked: 0,
					sdk: 0,
					unknown_external: 0,
					unresolved: 0,
					unresolved_reasons: {},
				},
			},
		},
	};
}
