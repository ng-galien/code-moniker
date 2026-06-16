const assert = require("node:assert");
const path = require("node:path");
const vscode = require("vscode");

const { collectSymbols, getApi, waitFor, waitForReady } = require("./helpers");

// Feature 2: navigate files → symbols, drive the reactive detail webview from
// selection without opening a file, then open source explicitly.
async function testSymbolTree() {
	const api = await getApi();
	await waitForReady(api);
	api.symbols.refresh();

	const topLevel = await waitFor(async () => {
		const nodes = await api.symbols.getChildren();
		return nodes.some((node) => node.kind === "entry") ? nodes : undefined;
	}, "top-level tree entries");

	const srcDir = topLevel.find(
		(node) => node.kind === "entry" && node.tree.kind === "directory" && node.tree.path === "src",
	);
	assert.ok(srcDir, "the seeded `src` directory should be listed");

	const srcChildren = await api.symbols.getChildren(srcDir);
	const libFile = srcChildren.find(
		(node) => node.kind === "entry" && node.tree.path === "src/lib.rs",
	);
	assert.ok(libFile, "`src/lib.rs` should be listed under src");

	const symbols = await collectSymbols(api.symbols, libFile);
	const names = symbols.map((node) => node.symbol.name);
	// Function symbol names carry a trailing "()"; match on the base name.
	const starts = (prefix) => names.some((name) => name.startsWith(prefix));
	assert.ok(starts("Widget"), `expected Widget among ${names.join(", ")}`);
	assert.ok(starts("DoThing"), `expected DoThing among ${names.join(", ")}`);
	assert.ok(
		starts("new") || starts("grow") || starts("build_widget"),
		`expected functions among ${names.join(", ")}`,
	);

	// Selecting a symbol must populate the reactive webview, not open a file.
	const doThing = symbols.find((node) => node.symbol.name.startsWith("DoThing"));
	const editorsBefore = vscode.window.visibleTextEditors.length;
	await api.detail.showForSymbol(doThing.symbol);
	await waitFor(
		() => api.detail.lastPayload && api.detail.lastPayload.symbol.name.startsWith("DoThing"),
		"detail webview to render the selected symbol",
	);
	assert.ok(api.detail.visible, "detail webview should be visible");
	assert.ok(api.detail.lastPayload.source, "detail payload should include a source snippet");
	assert.strictEqual(
		vscode.window.visibleTextEditors.length,
		editorsBefore,
		"selecting a symbol must not open an editor",
	);

	// Opening source is an explicit action.
	await vscode.commands.executeCommand("codeMoniker.symbols.openSource", doThing);
	const editor = await waitFor(
		() =>
			vscode.window.activeTextEditor &&
			vscode.window.activeTextEditor.document.uri.fsPath.endsWith(path.join("src", "lib.rs"))
				? vscode.window.activeTextEditor
				: undefined,
		"openSource to reveal src/lib.rs",
	);
	assert.ok(editor, "active editor should be lib.rs after openSource");

	console.log("symbol tree + detail: ok");
}

module.exports = { testSymbolTree };
