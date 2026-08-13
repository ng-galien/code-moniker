import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const extensionRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

async function loadTypeScript(relativeEntry) {
	const result = await build({
		absWorkingDir: extensionRoot,
		entryPoints: [relativeEntry],
		bundle: true,
		format: "esm",
		platform: "node",
		write: false,
	});
	const source = result.outputFiles[0].text;
	return import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
}

test("editor selection lookup is latest-wins across reversed async completions", async () => {
	const { LatestRequest } = await loadTypeScript("src/shared/latestRequest.ts");
	const latest = new LatestRequest();
	const applied = [];
	let resolveOld;
	let resolveNew;
	const oldResult = new Promise((resolve) => { resolveOld = resolve; });
	const newResult = new Promise((resolve) => { resolveNew = resolve; });
	const run = async (token, promise) => {
		const value = await promise;
		if (latest.isCurrent(token)) applied.push(value);
	};
	const oldWork = run(latest.begin(), oldResult);
	const newWork = run(latest.begin(), newResult);
	resolveNew("new symbol");
	await newWork;
	resolveOld("old symbol");
	await oldWork;
	assert.deepEqual(applied, ["new symbol"]);
});

test("a missing superseding tree reveal cannot leave selection blocked", async () => {
	const { RevealCoordinator } = await loadTypeScript("src/workbench/revealCoordinator.ts");
	const reveals = new RevealCoordinator();
	const oldReveal = reveals.begin("old");
	assert.equal(reveals.markProgrammatic(oldReveal), true);
	const missingReveal = reveals.begin("missing");
	reveals.finish(missingReveal);
	reveals.finish(oldReveal);
	assert.equal(reveals.consumeSelection("real-user-selection"), false);
	const nextReveal = reveals.begin("next");
	assert.equal(reveals.isCurrent(nextReveal), true);
});

test("undo or refocus invalidates an expansion response already in flight", async () => {
	const { ExpansionCoordinator } = await loadTypeScript("src/explorer/webview/expansionCoordinator.ts");
	const expansions = new ExpansionCoordinator();
	const stale = expansions.begin("neighbor", "root-a", "outgoing");
	expansions.reset();
	assert.equal(expansions.take(stale, "root-a"), undefined);
	const current = expansions.begin("neighbor", "root-b", "incoming");
	assert.equal(expansions.take(current, "root-a"), undefined);
	assert.equal(expansions.take(current, "root-b")?.direction, "incoming");
});

test("a multi-relation edge adopts the active relation visual", async () => {
	const { selectActiveEdgeRelations } = await loadTypeScript("src/explorer/webview/graph/edgeRelations.ts");
	const visual = selectActiveEdgeRelations(
		["calls", "data"],
		{ calls: "method call", data: "reads" },
		{
			incoming: true,
			outgoing: true,
			calls: false,
			data: true,
			types: true,
			references: true,
		},
	);
	assert.deepEqual(visual, { relation: "data", relations: ["data"], label: "reads" });
});

test("code neighborhoods expose six ranked, balanced and deduplicated relations", async () => {
	const {
		INITIAL_RELATION_BUDGET,
		rankCodeNeighbors,
		selectInitialCodeNeighbors,
	} = await loadTypeScript("src/explorer/webview/graph/neighborSelection.ts");
	const symbol = (name, file = "src/other.rs") => ({
		file,
		id: name,
		kind: "function",
		language: "rust",
		name,
		navigable: true,
		root: ".",
		signature: `${name}()`,
		uri: `code+moniker://${name}`,
		visibility: "public",
	});
	const focus = symbol("focus", "src/lib.rs");
	const row = (name, kinds, count = 1, file) => ({ symbol: symbol(name, file), kinds, count });
	const incoming = rankCodeNeighbors([
		row("reference-heavy", ["references"], 20),
		row("caller", ["calls"], 1, "src/lib.rs"),
		row("caller", ["reads"], 2, "src/lib.rs"),
		row("writer", ["writes"]),
	], focus);
	assert.equal(incoming[0].symbol.name, "caller", "calls in the same file should outrank raw reference volume");
	assert.deepEqual(incoming[0].kinds.sort(), ["calls", "reads"]);
	assert.equal(incoming.filter((candidate) => candidate.symbol.name === "caller").length, 1);
	const outgoing = rankCodeNeighbors([
		row("dep-a", ["calls"]),
		row("dep-b", ["writes"]),
		row("dep-c", ["reads"]),
		row("dep-d", ["uses_type"]),
		row("dep-e", ["references"]),
	], focus);
	const selected = selectInitialCodeNeighbors(incoming, outgoing, focus);
	const visible = [...selected.incoming, ...selected.outgoing];
	assert.equal(INITIAL_RELATION_BUDGET, 6);
	assert.equal(visible.length, 6);
	assert.ok(selected.incoming.length >= 2, "incoming relations should retain a direction floor");
	assert.ok(selected.outgoing.length >= 2, "outgoing relations should retain a direction floor");
	assert.equal(new Set(visible.map((candidate) => candidate.symbol.uri)).size, visible.length);
});
