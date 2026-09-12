import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { NodeDaemonRuntime } from "../dist/node.js";
import { assertSqlIndexRules } from "./assert-sql-index-rules.mjs";

if (!process.argv[2]) throw new Error("usage: smoke-sql-index-rules.mjs <code-moniker-binary>");
const workspaceRoot = mkdtempSync(join(tmpdir(), "code-moniker-sql-rules-"));
const registryDirectory = mkdtempSync(join(tmpdir(), "code-moniker-sql-registry-"));
const runtime = new NodeDaemonRuntime({ registryDirectory });
let owned;
let client;
try {
	owned = await runtime.launch({workspaceRoots: [workspaceRoot], binaryCandidates: [resolve(process.argv[2])]});
	client = await runtime.connect(owned.entry, {clientName: "sql-inline-rules-smoke"});
	await client.workspace.refresh();
	await assertSqlIndexRules(client);
	console.log("SQL workspace rules passed: statement partition/order invariance, incorrect prefix, index removal and replacement.");
} finally {
	client?.close();
	if (owned) await runtime.stopOwned(owned, {exitTimeoutMs: 10_000});
	rmSync(workspaceRoot, {recursive: true, force: true});
	rmSync(registryDirectory, {recursive: true, force: true});
}
