const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vscode = require("vscode");

const NOTEBOOK_TYPE = "code-moniker-scenario";
const TIMEOUT_MS = 15000;

async function run() {
	await activateExtension();
	assertCatalogTreeMenusContributed();
	await assertRendererOutputLinks();
	await configureBinary();
	await rejectCatalogGroupAsEntry();
	const editor = await openCatalogSample();
	assert.notStrictEqual(editor.notebook.uri.scheme, "untitled");
	assert.strictEqual(editor.notebook.isUntitled, false);
	assert.strictEqual(editor.notebook.isDirty, false);
	assert.match(path.basename(editor.notebook.uri.path), /^rust-naming(?:-\d+)?\.cm\.md$/);
	assertNoExpectCells(editor.notebook);
	await verifyNavigationCommands(editor);
	await runFileCell(editor);
	await runRulesCell(editor);
}

async function rejectCatalogGroupAsEntry() {
	await vscode.commands.executeCommand(
		"codeMoniker.catalog.openEntry",
		{ id: "builtin:packs" },
	);
}

async function activateExtension() {
	const extension = codeMonikerExtension();
	await extension.activate();
}

function assertCatalogTreeMenusContributed() {
	const packageJSON = codeMonikerExtension().packageJSON;
	const commands = packageJSON?.contributes?.commands || [];
	const menus = packageJSON?.contributes?.menus?.["view/item/context"] || [];
	assert.ok(
		hasMenuItem(
			menus,
			"codeMoniker.catalog.openEntry",
			"view == codeMoniker.catalog && viewItem =~ /^cmCatalog.*Entry$/",
		),
		"catalog tree should open catalog entry rows",
	);
	assert.ok(
		hasMenuItem(
			menus,
			"codeMoniker.catalog.openEntry",
			"view == codeMoniker.catalog && viewItem =~ /^cmCatalog.*Rule$/",
		),
		"catalog tree should open catalog rule rows",
	);
	assert.ok(
		!hasCommand(commands, "codeMoniker.catalog.copyToUserCatalog"),
		"catalog should not expose a user-catalog copy command",
	);
	assert.ok(
		!hasCommand(commands, "codeMoniker.catalog.openUserFolder"),
		"catalog should not expose a user catalog folder command",
	);
}

function hasMenuItem(menus, command, when) {
	return menus.some((item) => item.command === command && item.when === when);
}

function hasCommand(commands, command) {
	return commands.some((item) => item.command === command);
}

function codeMonikerExtension() {
	const extension = vscode.extensions.all.find((candidate) => candidate.packageJSON?.name === "code-moniker");
	assert.ok(extension, "Code Moniker extension should be installed in the test host");
	return extension;
}

async function configureBinary() {
	const binary = process.env.CODE_MONIKER_BINARY;
	assert.ok(binary, "CODE_MONIKER_BINARY must point to the test code-moniker binary");
	await vscode.workspace
		.getConfiguration("codeMoniker")
		.update("binaryPath", binary, vscode.ConfigurationTarget.Global);
}

async function runFileCell(editor) {
	const fileIndex = findCellIndex(editor.notebook, (meta) => meta.cmType === "file" && meta.path === "src/lib.rs");
	await executeCell(editor.notebook, fileIndex);
	const output = await waitForCellOutput(editor.notebook, fileIndex, "code-moniker check src/lib.rs");
	assert.match(output, /snake-case/);
}

async function runRulesCell(editor) {
	const rulesIndex = findCellIndex(editor.notebook, (meta) => meta.cmType === "rules");
	await executeCell(editor.notebook, rulesIndex);
	const output = await waitForCellOutput(editor.notebook, rulesIndex, "code-moniker check .");
	assert.match(output, /1 violation\(s\)/);
	assert.match(output, /src\/lib\.rs:L3/);
	const payload = checkOutputPayload(editor.notebook.cellAt(rulesIndex));
	assert.strictEqual(payload.kind, "check");
	assert.strictEqual(payload.files[0].file, "src/lib.rs");
	assert.strictEqual(payload.files[0].violations[0].rule_id, "rust.fn.snake-case");
	assert.deepStrictEqual(payload.files[0].violations[0].lines, [3, 3]);
}

async function openCatalogSample() {
	await vscode.commands.executeCommand(
		"codeMoniker.catalog.openEntry",
		{ id: "builtin:pack:rust-naming" },
	);
	return waitForScenarioEditor("editable catalog scenario notebook editor");
}

async function verifyNavigationCommands(editor) {
	const rulesIndex = findCellIndex(editor.notebook, (meta) => meta.cmType === "rules");
	const fileIndex = findCellIndex(editor.notebook, (meta) => meta.cmType === "file" && meta.path === "src/lib.rs");

	await vscode.commands.executeCommand("codeMoniker.scenario.revealRule", "rust.fn.snake-case");
	await waitForSelection(editor, rulesIndex, "rule cell selection");

	await vscode.commands.executeCommand("codeMoniker.scenario.revealFile", "src/lib.rs");
	await waitForSelection(editor, fileIndex, "file cell selection");

	await vscode.commands.executeCommand("codeMoniker.scenario.revealLine", "./src/lib.rs", 3);
	await waitForSelection(editor, fileIndex, "line cell selection");
	await waitForTextEditorLine(editor.notebook.cellAt(fileIndex), 3);
}

function findCellIndex(notebook, predicate) {
	for (let index = 0; index < notebook.cellCount; index++) {
		const meta = notebook.cellAt(index).metadata || {};
		if (predicate(meta)) {
			return index;
		}
	}
	throw new Error("Expected notebook cell was not found");
}

function assertNoExpectCells(notebook) {
	for (let index = 0; index < notebook.cellCount; index++) {
		const meta = notebook.cellAt(index).metadata || {};
		assert.notStrictEqual(
			meta.cmType,
			"expect",
			"cm:expect blocks are test assertions and must not be rendered as notebook cells",
		);
	}
}

async function executeCell(notebook, index) {
	await clearCellOutput(notebook, index);
	await vscode.commands.executeCommand("codeMoniker.scenario.executeCell", index);
}

async function clearCellOutput(notebook, index) {
	await vscode.commands.executeCommand("notebook.cell.clearOutputs", {
		document: notebook.uri,
		ranges: [new vscode.NotebookRange(index, index + 1)],
	});
}

async function waitForCellOutput(notebook, index, expected) {
	let lastOutput = "";
	return waitFor(() => {
		const text = outputText(notebook.cellAt(index));
		lastOutput = text;
		return text.includes(expected) ? text : undefined;
	}, `output containing ${expected}`, () => lastOutput);
}

function outputText(cell) {
	const decoder = new TextDecoder();
	return cell.outputs
		.flatMap((output) => output.items)
		.map((item) => decoder.decode(item.data))
		.join("\n");
}

function checkOutputPayload(cell) {
	const decoder = new TextDecoder();
	for (const output of cell.outputs) {
		for (const item of output.items) {
			if (item.mime === "application/x-code-moniker-violations+json") {
				return JSON.parse(decoder.decode(item.data));
			}
		}
	}
	throw new Error("Expected check output JSON payload was not found");
}

async function assertRendererOutputLinks() {
	const restoreDocument = installTestDocument();
	try {
		const rendererPath = path.resolve(__dirname, "..", "..", "dist", "renderer.js");
		const source = fs.readFileSync(rendererPath, "utf8");
		const moduleUrl = `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
		const renderer = await import(moduleUrl);
		const messages = [];
		const api = renderer.activate({ postMessage: (message) => messages.push(message) });
		const host = document.createElement("div");
		api.renderOutputItem({ json: () => rendererPayload }, host);

		const links = findElements(host, (node) => hasClass(node, "cm-link"));
		assert.deepStrictEqual(
			links.map((link) => link.textContent),
			["src/lib.rs", "L3", "rust.fn.snake-case"],
			"renderer output should expose file, line, and rule navigation links",
		);
		for (const link of links) {
			assert.strictEqual(link.disabled, false, `${link.textContent} should be enabled`);
		}

		links[0].click();
		links[1].click();
		links[2].click();
		assert.deepStrictEqual(messages, [
			{ command: "revealFile", file: "src/lib.rs" },
			{ command: "revealLine", file: "src/lib.rs", line: 3 },
			{ command: "revealRule", ruleId: "rust.fn.snake-case" },
		]);
	} finally {
		restoreDocument();
	}
}

async function waitForScenarioEditor(label) {
	return waitFor(
		() => {
			const active = vscode.window.activeNotebookEditor;
			return active?.notebook.notebookType === NOTEBOOK_TYPE ? active : undefined;
		},
		label,
	);
}

async function waitForSelection(editor, index, label) {
	await waitFor(
		() => editor.selection.start === index ? true : undefined,
		label,
		() => `selected ${editor.selection.start}, expected ${index}`,
	);
}

async function waitForTextEditorLine(cell, line) {
	const expectedLine = line - 1;
	await waitFor(
		() => {
			const editor = vscode.window.activeTextEditor;
			return editor?.document.uri.toString() === cell.document.uri.toString()
				&& editor.selection.active.line === expectedLine
				? true
				: undefined;
		},
		`cell text editor line ${line}`,
		() => {
			const editor = vscode.window.activeTextEditor;
			return editor
				? `${editor.document.uri.toString()} line ${editor.selection.active.line + 1}`
				: "no active text editor";
		},
	);
}

async function waitFor(probe, label, details = () => "") {
	const deadline = Date.now() + TIMEOUT_MS;
	let lastError;
	while (Date.now() < deadline) {
		try {
			const value = probe();
			if (value) {
				return value;
			}
		} catch (error) {
			lastError = error;
		}
		await delay(100);
	}
	const suffix = lastError ? ` Last error: ${lastError.message}` : "";
	const detail = details();
	const detailSuffix = detail ? ` Last output:\n${detail}` : "";
	throw new Error(`Timed out waiting for ${label}.${suffix}${detailSuffix}`);
}

function delay(ms) {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

const rendererPayload = {
	kind: "check",
	target: ".",
	summary: {
		files_scanned: 1,
		files_with_violations: 1,
		total_violations: 1,
		total_errors: 1,
		total_warnings: 0,
	},
	files: [
		{
			file: "src/lib.rs",
			violations: [
				{
					rule_id: "rust.fn.snake-case",
					severity: "error",
					moniker: "code+moniker://workspace/src/lib.rs#fn.DoThing",
					kind: "function",
					lines: [3, 3],
					message: "expected snake_case",
					explanation: "Function names should use snake_case.",
				},
			],
		},
	],
};

function installTestDocument() {
	const previous = globalThis.document;
	const elementsById = new Map();
	const document = {
		head: undefined,
		createElement(tagName) {
			return new TestElement(tagName, elementsById);
		},
		getElementById(id) {
			return elementsById.get(id) || null;
		},
	};
	document.head = document.createElement("head");
	globalThis.document = document;
	return () => {
		if (previous === undefined) {
			delete globalThis.document;
		} else {
			globalThis.document = previous;
		}
	};
}

class TestElement {
	constructor(tagName, elementsById) {
		this.tagName = tagName.toUpperCase();
		this.children = [];
		this.className = "";
		this.textContent = "";
		this.title = "";
		this.type = "";
		this.disabled = false;
		this.attributes = new Map();
		this.listeners = new Map();
		this.elementsById = elementsById;
		this.classList = {
			add: (...names) => {
				const classes = new Set(this.className.split(/\s+/).filter(Boolean));
				for (const name of names) {
					classes.add(name);
				}
				this.className = [...classes].join(" ");
			},
		};
	}

	set id(value) {
		this._id = value;
		if (value) {
			this.elementsById.set(value, this);
		}
	}

	get id() {
		return this._id;
	}

	set innerHTML(value) {
		this._innerHTML = value;
		this.textContent = String(value).replace(/<[^>]*>/g, "");
	}

	get innerHTML() {
		return this._innerHTML || "";
	}

	appendChild(child) {
		this.children.push(child);
		child.parentElement = this;
		if (child.id) {
			this.elementsById.set(child.id, child);
		}
		return child;
	}

	replaceChildren(...children) {
		this.children = [];
		for (const child of children) {
			this.appendChild(child);
		}
	}

	setAttribute(name, value) {
		this.attributes.set(name, String(value));
	}

	addEventListener(name, listener) {
		const listeners = this.listeners.get(name) || [];
		listeners.push(listener);
		this.listeners.set(name, listeners);
	}

	click() {
		const event = {
			preventDefault() {},
			stopPropagation() {},
		};
		for (const listener of this.listeners.get("click") || []) {
			listener(event);
		}
	}
}

function findElements(root, predicate) {
	const found = [];
	const visit = (node) => {
		if (predicate(node)) {
			found.push(node);
		}
		for (const child of node.children || []) {
			visit(child);
		}
	};
	visit(root);
	return found;
}

function hasClass(node, className) {
	return node.className.split(/\s+/).includes(className);
}

module.exports = { run };
