import * as vscode from "vscode";

import { LANGS, LangDef, langByVscodeId } from "../shared/languages";
import { CmnbCell } from "./model";
import { ruleContext, selectedRuleCell } from "./cells";
import { sampleText } from "./samples";
import { toCellData } from "./serializer";

const NOTEBOOK_TYPE = "code-moniker";

// Command: create a starter rule notebook for a chosen language.
export async function newNotebook(): Promise<void> {
	const pick = await vscode.window.showQuickPick(
		LANGS.map((lang) => ({ label: lang.label, lang })),
		{ title: "New Code Moniker Rule Notebook", placeHolder: "Pick a language" },
	);
	if (!pick) {
		return;
	}
	const cells = starterCells(pick.lang).map(toCellData);
	const data = new vscode.NotebookData(cells);
	const notebook = await vscode.workspace.openNotebookDocument(NOTEBOOK_TYPE, data);
	await vscode.window.showNotebookDocument(notebook);
}

// Command: add a sample code cell below the selected cell.
export async function addSampleCell(anchor?: vscode.NotebookCell): Promise<void> {
	const notebook = notebookFor(anchor);
	if (!notebook) {
		return;
	}
	const lang = await pickLanguage(notebook, anchor);
	if (!lang) {
		return;
	}
	const cell: CmnbCell = {
		kind: "sample",
		language: lang.id,
		value: `// ${lang.label} sample to test rules against\n`,
	};
	await insertBelow(notebook, cell, anchor);
}

// Command: add a rule cell (a real .code-moniker.toml fragment) below the selection.
export async function addRuleCell(anchor?: vscode.NotebookCell): Promise<void> {
	const notebook = notebookFor(anchor);
	if (!notebook) {
		return;
	}
	const lang = await pickLanguage(notebook, anchor);
	if (!lang) {
		return;
	}
	await insertBelow(notebook, { kind: "rule", language: lang.id, value: sizeRule(lang) }, anchor);
}

export async function copyRuleToProjectConfig(
	cell?: vscode.NotebookCell,
): Promise<void> {
	const target = cell ?? selectedRuleCell();
	const rule = target ? ruleContext(target) : undefined;
	if (!rule) {
		void vscode.window.showWarningMessage("Select a Code Moniker rule cell first.");
		return;
	}
	const folder = vscode.workspace.workspaceFolders?.[0];
	if (!folder) {
		void vscode.window.showWarningMessage(
			"Open a workspace folder before copying a rule to project config.",
		);
		return;
	}
	const config = vscode.Uri.joinPath(folder.uri, ".code-moniker.toml");
	const fragment = projectRuleFragment(rule.rulesToml);
	if (!fragment) {
		void vscode.window.showWarningMessage("The rule cell does not contain a rule fragment.");
		return;
	}
	const existing = await readText(config);
	const next = existing
		? `${existing.trimEnd()}\n\n${fragment}\n`
		: `default_rules = false\n\n${fragment}\n`;
	await vscode.workspace.fs.writeFile(config, new TextEncoder().encode(next));
	void vscode.window.showInformationMessage(
		`Copied rule to ${vscode.workspace.asRelativePath(config)}.`,
	);
}

async function pickLanguage(
	notebook?: vscode.NotebookDocument,
	anchor?: vscode.NotebookCell,
): Promise<LangDef | undefined> {
	const guessed = notebook ? guessLanguage(notebook, anchor) : undefined;
	if (guessed) {
		return guessed;
	}
	const pick = await vscode.window.showQuickPick(
		LANGS.map((lang) => ({ label: lang.label, lang })),
		{ placeHolder: "Pick a language" },
	);
	return pick?.lang;
}

function guessLanguage(
	notebook: vscode.NotebookDocument,
	anchor?: vscode.NotebookCell,
): LangDef | undefined {
	const start = anchor?.index ?? vscode.window.activeNotebookEditor?.selection.start ?? notebook.cellCount - 1;
	for (let index = Math.min(start, notebook.cellCount - 1); index >= 0; index--) {
		const cell = notebook.cellAt(index);
		const meta = cell.metadata as { language?: string } | undefined;
		if (meta?.language) {
			return LANGS.find((lang) => lang.id === meta.language);
		}
		const byEditor = langByVscodeId(cell.document.languageId);
		if (byEditor) {
			return byEditor;
		}
	}
	return undefined;
}

async function insertBelow(
	notebook: vscode.NotebookDocument,
	cell: CmnbCell,
	anchor?: vscode.NotebookCell,
): Promise<void> {
	const index = anchor ? anchor.index + 1 : vscode.window.activeNotebookEditor?.selection.end ?? notebook.cellCount;
	const edit = new vscode.WorkspaceEdit();
	edit.set(notebook.uri, [
		vscode.NotebookEdit.insertCells(index, [toCellData(cell)]),
	]);
	await vscode.workspace.applyEdit(edit);
}

// A universal starter rule: every supported language has callables.
function sizeRule(lang: LangDef): string {
	return (
		"default_rules = false\n\n" +
		`[[${lang.tomlSection}.shape.callable.where]]\n` +
		'id        = "max-lines"\n' +
		'expr      = "lines <= 6"\n' +
		'severity  = "warn"\n' +
		'message   = "{kind} `{name}` is {value} lines (cap {expected})."\n' +
		'rationale = "Short callables are easier to read, test, and reuse."\n'
	);
}

function starterCells(lang: LangDef): CmnbCell[] {
	return [
		{
			kind: "markdown",
			value:
				`# ${lang.label} rule notebook\n\n` +
				"This notebook teaches **code-moniker check rules**. A **rule cell** is a real " +
				"`.code-moniker.toml` fragment — exactly what you paste into your project. Run the " +
				"**sample cell** (▷) to check it against the rule cells below, and run a rule cell " +
				"to validate the TOML fragment.\n\n" +
				"Edit the sample, `expr`, `message`, or `severity` and re-run the sample to learn by experiment.",
		},
		{
			kind: "markdown",
			value:
				"## Keep callables small\n\n" +
				"A symbol *violates* a rule when its `expr` is **false** for it. Lower the cap and re-run.",
		},
		sampleFor(lang),
		{ kind: "rule", language: lang.id, value: sizeRule(lang) },
	];
}

function sampleFor(lang: LangDef): CmnbCell {
	return { kind: "sample", language: lang.id, value: sampleText(lang.id) };
}

function notebookFor(
	anchor?: vscode.NotebookCell,
): vscode.NotebookDocument | undefined {
	return anchor?.notebook ?? vscode.window.activeNotebookEditor?.notebook;
}

async function readText(uri: vscode.Uri): Promise<string | undefined> {
	try {
		const bytes = await vscode.workspace.fs.readFile(uri);
		return new TextDecoder().decode(bytes);
	} catch {
		return undefined;
	}
}

function projectRuleFragment(rulesToml: string): string {
	return rulesToml
		.split("\n")
		.filter((line) => !/^\s*default_rules\s*=/.test(line))
		.join("\n")
		.trim();
}
