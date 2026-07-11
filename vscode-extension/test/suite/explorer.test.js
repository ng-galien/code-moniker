const assert = require("node:assert");
const path = require("node:path");
const vscode = require("vscode");

const { getApi, waitForReady, waitFor } = require("./helpers");

// End-to-end acceptance for the Graph Explorer: replay the user's two entry
// gestures and require an ack from inside the webview. The ack is posted by
// the React bundle after it applies a scope, so a green run proves the whole
// chain: command -> daemon query -> host message -> webview render.
async function testExplorer() {
	const api = await waitForReady(await getApi());
	const explorer = api.explorer;
	assert.ok(explorer?.panel, "extension api should expose the explorer panel");

	// Gesture 1: right-click a Symbols row -> Open in Graph Explorer. The
	// context menu hands the command the workspace tree element verbatim, so
	// the test passes exactly that wrapped node.
	const sections = await api.workspace.getChildren(undefined);
	const symbolsSection = sections.find(
		(node) => node.kind === "section" && node.id === "symbols",
	);
	assert.ok(symbolsSection, "workspace tree should expose a Symbols section");
	const rows = await waitFor(async () => {
		const children = await api.workspace.getChildren(symbolsSection);
		const usable = children.filter((node) => node.kind === "symbols");
		return usable.length > 0 ? usable : undefined;
	}, "Symbols section rows");
	await vscode.commands.executeCommand("codeMoniker.explorer.focus", rows[0]);
	const treeAck = await waitFor(
		() => explorer.panel.webviewAcks[0],
		"the explorer webview to ack the tree-focused scope",
	);
	assert.ok(
		treeAck.nodes > 0,
		`tree focus should draw a populated scope, got ${JSON.stringify(treeAck)}; ` +
			`last host message: ${JSON.stringify(explorer.panel.current)?.slice(0, 300)}`,
	);
	console.log(`explorer tree focus: ok (${treeAck.nodes} nodes at "${treeAck.prefix}")`);

	// Gesture 2: cursor inside a definition -> Focus Symbol at Cursor.
	const workspaceRoot = process.env.CODE_MONIKER_TEST_WORKSPACE;
	assert.ok(workspaceRoot, "CODE_MONIKER_TEST_WORKSPACE must point to the test workspace");
	const document = await vscode.workspace.openTextDocument(
		path.join(workspaceRoot, "src", "lib.rs"),
	);
	const editor = await vscode.window.showTextDocument(document);
	const line = document
		.getText()
		.split("\n")
		.findIndex((text) => text.includes("fn "));
	assert.ok(line >= 0, "seeded src/lib.rs should contain a function");
	editor.selection = new vscode.Selection(line, 3, line, 3);
	const before = explorer.panel.webviewAcks.length;
	await vscode.commands.executeCommand("codeMoniker.explorer.focusAtCursor");
	const cursorAck = await waitFor(
		() => explorer.panel.webviewAcks[before],
		"the explorer webview to ack the cursor-focused scope",
	);
	assert.ok(
		cursorAck.nodes > 0,
		`cursor focus should draw a populated scope, got ${JSON.stringify(cursorAck)}; ` +
			`last host message: ${JSON.stringify(explorer.panel.current)?.slice(0, 300)}`,
	);
	console.log(
		`explorer cursor focus: ok (${cursorAck.nodes} nodes at "${cursorAck.prefix}")`,
	);

	// Gesture 3: click a definition card -> code inset. The harness cannot
	// click inside a webview, so the test drives the same host path the
	// inspect message takes (panel.inspect) and requires the webview to ack
	// an inset carrying highlighted source lines.
	const scopeMessage = explorer.panel.current;
	assert.strictEqual(scopeMessage?.type, "scope", "panel should hold the last scope");
	const def = scopeMessage.payload.graph.nodes.find((node) => node.symbol);
	assert.ok(def, "the cursor-focused scope should contain definition nodes");
	await explorer.panel.inspect(def.symbol.uri);
	const insetAck = await waitFor(
		() => explorer.panel.insetAcks[0],
		"the explorer webview to ack a code inset",
	);
	assert.ok(
		insetAck.lines > 0,
		`the inset should carry source lines, got ${JSON.stringify(insetAck)}`,
	);
	console.log(`explorer code inset: ok (${insetAck.lines} lines for ${def.symbol.name})`);
}

module.exports = { testExplorer };
