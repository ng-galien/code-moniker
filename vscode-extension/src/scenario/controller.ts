import * as vscode from "vscode";

import { runScenario } from "../cli/facade";
import { LANGS } from "../shared/languages";
import { serializeScenarioMarkdown } from "./markdown";
import { notebookToScenario } from "./serializer";
import { SCENARIO_NOTEBOOK_TYPE, ScenarioCellMeta } from "./model";

const VIOLATION_LINE = /^(\S+) @ (.+?):L(\d+)(?:-L(\d+))?$/;

// Kernel for scenario notebooks: any execution replays the WHOLE document
// through `code-moniker check --scenario -` and maps the observed violations
// back onto the matching file cells as diagnostics.
export class ScenarioController {
	private readonly controller: vscode.NotebookController;
	private readonly diagnostics: vscode.DiagnosticCollection;

	constructor() {
		this.diagnostics = vscode.languages.createDiagnosticCollection(
			"code-moniker-scenario",
		);
		this.controller = vscode.notebooks.createNotebookController(
			"code-moniker-scenario-kernel",
			SCENARIO_NOTEBOOK_TYPE,
			"Code Moniker Scenario",
		);
		this.controller.supportedLanguages = [
			"cmrule-toml",
			"plaintext",
			...LANGS.map((lang) => lang.vscodeId),
		];
		this.controller.description = "Replay the scenario and verify its expectations.";
		this.controller.executeHandler = this.execute.bind(this);
	}

	dispose(): void {
		this.diagnostics.dispose();
		this.controller.dispose();
	}

	private async execute(
		cells: vscode.NotebookCell[],
		notebook: vscode.NotebookDocument,
	): Promise<void> {
		const cell = cells[0] ?? notebook.cellAt(0);
		const execution = this.controller.createNotebookCellExecution(cell);
		execution.start(Date.now());
		const document = serializeScenarioMarkdown(
			notebookToScenario(notebookData(notebook)),
		);
		const result = await runScenario(document);
		if (!result.ok) {
			await execution.replaceOutput(textOutput(result.error));
			execution.end(false, Date.now());
			return;
		}
		this.publishDiagnostics(notebook, result.output);
		await execution.replaceOutput(textOutput(result.output));
		execution.end(result.matched, Date.now());
	}

	private publishDiagnostics(notebook: vscode.NotebookDocument, output: string): void {
		clearNotebookDiagnostics(this.diagnostics, notebook);
		for (const [path, diagnostics] of violationsByPath(output)) {
			const cell = fileCell(notebook, path);
			if (cell) {
				this.diagnostics.set(cell.document.uri, diagnostics);
			}
		}
	}
}

function notebookData(notebook: vscode.NotebookDocument): vscode.NotebookData {
	const cells = notebook.getCells().map((cell) => {
		const data = new vscode.NotebookCellData(
			cell.kind,
			cell.document.getText(),
			cell.document.languageId,
		);
		data.metadata = cell.metadata;
		return data;
	});
	const data = new vscode.NotebookData(cells);
	data.metadata = notebook.metadata;
	return data;
}

function violationsByPath(output: string): Map<string, vscode.Diagnostic[]> {
	const map = new Map<string, vscode.Diagnostic[]>();
	for (const line of output.split("\n")) {
		const match = VIOLATION_LINE.exec(line.trim());
		if (!match) {
			continue;
		}
		const [, ruleId, path, start, end] = match;
		const range = new vscode.Range(
			Number(start) - 1,
			0,
			Number(end ?? start) - 1,
			Number.MAX_SAFE_INTEGER,
		);
		const diagnostic = new vscode.Diagnostic(
			range,
			ruleId,
			vscode.DiagnosticSeverity.Warning,
		);
		diagnostic.source = "code-moniker scenario";
		map.set(path, [...(map.get(path) ?? []), diagnostic]);
	}
	return map;
}

function fileCell(
	notebook: vscode.NotebookDocument,
	path: string,
): vscode.NotebookCell | undefined {
	return notebook.getCells().find((cell) => {
		const meta = cell.metadata as Partial<ScenarioCellMeta> | undefined;
		return meta?.cmType === "file" && meta.path === path;
	});
}

function clearNotebookDiagnostics(
	collection: vscode.DiagnosticCollection,
	notebook: vscode.NotebookDocument,
): void {
	for (const cell of notebook.getCells()) {
		collection.delete(cell.document.uri);
	}
}

function textOutput(text: string): vscode.NotebookCellOutput {
	return new vscode.NotebookCellOutput([
		vscode.NotebookCellOutputItem.text(text, "text/plain"),
	]);
}
