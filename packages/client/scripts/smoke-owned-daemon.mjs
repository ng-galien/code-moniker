import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import process from "node:process";

import { NodeDaemonRuntime } from "../dist/node.js";
import {
	assertDaemonWorkspaceIndexed,
	assertPostReadyMutation,
	seedDaemonWorkspace,
} from "./seed-daemon-workspace.mjs";

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
const registryDirectory = join(workspaceRoot, "custom-registry");
const runtime = new NodeDaemonRuntime({ registryDirectory });
let owned;
let client;

try {
	seedDaemonWorkspace(workspaceRoot);
	owned = await runtime.launch({
		workspaceRoots: [workspaceRoot],
		binaryCandidates: [binary],
	});
	client = await runtime.connect(owned.entry, {
		clientName: "@code-moniker/client-owned-smoke",
	});
	const initialStatus = await waitForSmokeWorkspace(client);
	assertDaemonWorkspaceIndexed(initialStatus, "owned Windows daemon");
	const mutationGeneration = await assertPostReadyMutation(
		client,
		workspaceRoot,
		initialStatus,
		"owned Windows daemon",
	);

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

	const coldPid = owned.entry.pid;
	client.close();
	client = undefined;
	await runtime.stop(owned.entry, { exitTimeoutMs: 10_000 });
	assertStoppedAndUnclaimed(runtime, owned, workspaceRoot, "cold owned daemon");
	owned = undefined;

	owned = await runtime.launch({
		workspaceRoots: [workspaceRoot],
		binaryCandidates: [binary],
	});
	client = await runtime.connect(owned.entry, {
		clientName: "@code-moniker/client-owned-warm-smoke",
	});
	const warmStatus = await waitForSmokeWorkspace(client);
	assertDaemonWorkspaceIndexed(warmStatus, "warm owned Windows daemon");
	const warmPid = owned.entry.pid;
	client.close();
	client = undefined;
	await runtime.stop(owned.entry, { exitTimeoutMs: 10_000 });
	assertStoppedAndUnclaimed(runtime, owned, workspaceRoot, "warm owned daemon");
	owned = undefined;

	console.log(
		`owned daemon smoke passed: cold pid ${coldPid}, warm pid ${warmPid}, registry ${runtime.registryDirectory}, ${initialStatus.files} files, ${initialStatus.symbols} symbols, ${initialStatus.references} references, ${initialStatus.timings?.total_ms ?? "unknown"} ms, mutation generation ${mutationGeneration}, graph focus ${view.uri}`,
	);
} finally {
	client?.close();
	if (owned) {
		await runtime.stopOwned(owned, {
			exitTimeoutMs: 10_000,
		});
	}
	rmSync(workspaceRoot, { recursive: true, force: true });
	if (existsSync(workspaceRoot)) {
		throw new Error(`owned daemon workspace cleanup failed: ${workspaceRoot}`);
	}
}

function assertStoppedAndUnclaimed(runtime, owned, workspaceRoot, label) {
	if (owned.process.isRunning()) {
		throw new Error(`${label} process ${owned.entry.pid} is still running`);
	}
	if (runtime.findDaemon([workspaceRoot])) {
		throw new Error(`${label} retained its daemon registry claim`);
	}
}

async function waitForSmokeWorkspace(client) {
	const deadline = Date.now() + 60_000;
	while (Date.now() <= deadline) {
		const status = await client.workspace.status();
		if (
			status.phase === "ready" &&
			typeof status.generation === "number" &&
			status.generation >= 1
		) {
			return status;
		}
		if (status.phase === "failed") {
			throw new Error(
				status.failure?.message ?? "owned daemon failed to load its workspace",
			);
		}
		await new Promise((resolveDelay) => setTimeout(resolveDelay, 50));
	}
	throw new Error("owned daemon smoke workspace did not become ready");
}
