import * as vscode from "vscode";

import { CheckOutputPayload, CheckReport, RendererMessage } from "../cli/model";
import {
	groupVisibleViolations,
	lineRangeLabel,
	severityCounts,
	visibleViolationDetail,
} from "../cli/presentation";
import { runScenarioCheck } from "../cli/facade";
import { toDiagnostic } from "../diagnostics/vscode";
import { scenarioControllerLanguages } from "../shared/languages";
import { serializeScenarioMarkdown } from "./markdown";
import { notebookToScenario } from "./serializer";
import { SCENARIO_NOTEBOOK_TYPE, ScenarioCellMeta } from "./model";

const CHECK_OUTPUT_MIME = "application/x-code-moniker-violations+json";
const CHECK_OUTPUT_RENDERER_ID = "code-moniker-violations";

// Kernel for scenario notebooks: file cells run a file-scoped check, while rule
// cells run a workspace-scoped check over the in-memory scenario. `cm:expect`
// blocks stay in notebook metadata for CLI scenario tests and are not rendered
// as notebook cells.
export class ScenarioController {
	private readonly controller: vscode.NotebookController;
	private readonly diagnostics: vscode.DiagnosticCollection;
	private readonly output: vscode.OutputChannel;
	private readonly cellStatusProvider: ScenarioCellStatusBarProvider;
	private readonly statusBarProvider: vscode.Disposable;
	private readonly rendererMessaging: vscode.NotebookRendererMessaging;
	private readonly rendererMessagingSubscription: vscode.Disposable;
	private readonly notebookOpenSubscription: vscode.Disposable;
	private readonly commandDisposables: vscode.Disposable[];

	constructor() {
		this.output = vscode.window.createOutputChannel("Code Moniker Scenario");
		this.diagnostics = vscode.languages.createDiagnosticCollection(
			"code-moniker-scenario",
		);
		this.controller = vscode.notebooks.createNotebookController(
			"code-moniker-scenario-kernel",
			SCENARIO_NOTEBOOK_TYPE,
			"Code Moniker Scenario",
		);
		this.controller.supportedLanguages = scenarioControllerLanguages();
		this.controller.description = "Run code-moniker check on the current scenario file or workspace.";
		this.controller.executeHandler = this.execute.bind(this);
		for (const notebook of vscode.workspace.notebookDocuments) {
			this.updateNotebookAffinity(notebook);
		}
		this.notebookOpenSubscription = vscode.workspace.onDidOpenNotebookDocument(
			(notebook) => this.updateNotebookAffinity(notebook),
		);
		this.cellStatusProvider = new ScenarioCellStatusBarProvider();
		this.statusBarProvider =
			vscode.notebooks.registerNotebookCellStatusBarItemProvider(
				SCENARIO_NOTEBOOK_TYPE,
				this.cellStatusProvider,
			);
		this.rendererMessaging =
			vscode.notebooks.createRendererMessaging(CHECK_OUTPUT_RENDERER_ID);
		this.rendererMessagingSubscription = this.rendererMessaging.onDidReceiveMessage(
			(event) => {
				void this.handleRendererMessage(event.editor, event.message);
			},
		);
		this.commandDisposables = [
			vscode.commands.registerCommand(
				"codeMoniker.scenario.executeCell",
				(index?: number) => this.executeActiveCell(index),
			),
			vscode.commands.registerCommand(
				"codeMoniker.scenario.revealFile",
				(file: string) => this.revealActiveFile(file),
			),
			vscode.commands.registerCommand(
				"codeMoniker.scenario.revealLine",
				(file: string, line: number) => this.revealActiveFile(file, line),
			),
			vscode.commands.registerCommand(
				"codeMoniker.scenario.revealRule",
				(ruleId: string) => this.revealActiveRule(ruleId),
			),
		];
	}

	dispose(): void {
		this.output.dispose();
		this.diagnostics.dispose();
		this.controller.dispose();
		this.cellStatusProvider.dispose();
		this.statusBarProvider.dispose();
		this.rendererMessagingSubscription.dispose();
		this.notebookOpenSubscription.dispose();
		for (const disposable of this.commandDisposables) {
			disposable.dispose();
		}
	}

	private async execute(
		cells: vscode.NotebookCell[],
		notebook: vscode.NotebookDocument,
	): Promise<void> {
		const requested = cells.length ? cells : [notebook.cellAt(0)];
		const runnable = runnableCells(requested);
		const executions = requested
			.filter((cell) => cell.kind === vscode.NotebookCellKind.Code)
			.map((cell) => this.controller.createNotebookCellExecution(cell));
		for (const execution of executions) {
			execution.start(Date.now());
		}
		this.cellStatusProvider.clearNotebook(notebook);
		if (runnable.length === 0) {
			await Promise.all(executions.map((execution) => execution.clearOutput()));
			endAll(executions, true);
			return;
		}
		const outputCell = outputCellFor(runnable);
		const scenario = notebookToScenario(notebookData(notebook));
		const targetFile = requested.length === 1 ? targetFilePath(outputCell) : undefined;
		const result = await runScenarioCheck({
			document: serializeScenarioMarkdown(scenario),
			targetFile,
		});
		if (!result.ok) {
			await replaceOutput(outputCell, result.error, undefined, executions);
			endAll(executions, false);
			return;
		}
		this.publishDiagnostics(notebook, result.report);
		await replaceOutput(outputCell, result.report, result.target, executions);
		endAll(executions, checkSucceeded(result.report));
	}

	private publishDiagnostics(notebook: vscode.NotebookDocument, report: CheckReport): void {
		clearNotebookDiagnostics(this.diagnostics, notebook);
		for (const file of report.files) {
			const cell = fileCell(notebook, file.file);
			if (cell) {
				this.diagnostics.set(cell.document.uri, file.violations.map(toDiagnostic));
				this.cellStatusProvider.setResult(cell.document.uri, file.violations);
			}
		}
	}

	private async handleRendererMessage(
		editor: vscode.NotebookEditor,
		message: unknown,
	): Promise<void> {
		if (editor.notebook.notebookType !== SCENARIO_NOTEBOOK_TYPE) {
			this.output.appendLine("Ignored renderer message from a non-scenario notebook.");
			return;
		}
		if (!isCheckOutputRendererMessage(message)) {
			this.output.appendLine(`Ignored invalid renderer message: ${JSON.stringify(message)}`);
			return;
		}
		if (message.command === "revealRule") {
			await this.revealRule(editor, message.ruleId);
			return;
		}
		if (message.command === "revealLine") {
			await this.revealFile(editor, message.file, message.line);
			return;
		}
		await this.revealFile(editor, message.file);
	}

	private updateNotebookAffinity(notebook: vscode.NotebookDocument): void {
		if (notebook.notebookType === SCENARIO_NOTEBOOK_TYPE) {
			this.controller.updateNotebookAffinity(
				notebook,
				vscode.NotebookControllerAffinity.Preferred,
			);
		}
	}

	private async executeActiveCell(index?: number): Promise<void> {
		const editor = activeScenarioEditor();
		if (!editor) {
			return;
		}
		const target = index ?? editor.selection.start;
		if (target < 0 || target >= editor.notebook.cellCount) {
			return;
		}
		await this.executeDirect([editor.notebook.cellAt(target)], editor.notebook);
	}

	private async executeDirect(
		cells: vscode.NotebookCell[],
		notebook: vscode.NotebookDocument,
	): Promise<void> {
		const runnable = runnableCells(cells);
		if (runnable.length === 0) {
			await replaceOutputsDirect(notebook, cells, undefined, undefined);
			return;
		}
		const outputCell = outputCellFor(runnable);
		const scenario = notebookToScenario(notebookData(notebook));
		const targetFile = cells.length === 1 ? targetFilePath(outputCell) : undefined;
		const result = await runScenarioCheck({
			document: serializeScenarioMarkdown(scenario),
			targetFile,
		});
		if (!result.ok) {
			await replaceOutputsDirect(notebook, cells, outputCell, textOutput(result.error));
			return;
		}
		this.publishDiagnostics(notebook, result.report);
		await replaceOutputsDirect(
			notebook,
			cells,
			outputCell,
			checkOutput(result.report, result.target),
		);
		await nextTick();
		this.publishDiagnostics(notebook, result.report);
	}

	private async revealActiveFile(file: string, line?: number): Promise<void> {
		const editor = activeScenarioEditor();
		if (editor) {
			await this.revealFile(editor, file, line);
			return;
		}
		this.warnNavigation("No active Code Moniker scenario notebook.");
	}

	private async revealActiveRule(ruleId: string): Promise<void> {
		const editor = activeScenarioEditor();
		if (editor) {
			await this.revealRule(editor, ruleId);
			return;
		}
		this.warnNavigation("No active Code Moniker scenario notebook.");
	}

	private async revealFile(
		editor: vscode.NotebookEditor,
		file: string,
		line?: number,
	): Promise<void> {
		const cell = fileCell(editor.notebook, file);
		if (!cell) {
			this.warnNavigation(`Scenario file "${file}" was not found in this notebook.`);
			return;
		}
		await revealNotebookCell(editor, cell.index);
		if (line !== undefined) {
			await revealCellLine(editor, cell, line, this.output);
		}
	}

	private async revealRule(
		editor: vscode.NotebookEditor,
		ruleId: string,
	): Promise<void> {
		const index = ruleCellIndex(editor.notebook, ruleId);
		if (index === undefined) {
			this.warnNavigation(`Scenario rule "${ruleId}" was not found in this notebook.`);
			return;
		}
		await revealNotebookCell(editor, index);
	}

	private warnNavigation(message: string): void {
		this.output.appendLine(message);
		void vscode.window.showWarningMessage(message);
	}
}

class ScenarioCellStatusBarProvider implements vscode.NotebookCellStatusBarItemProvider {
	private readonly changed = new vscode.EventEmitter<void>();
	private readonly results = new Map<string, CellResultStatus>();

	readonly onDidChangeCellStatusBarItems = this.changed.event;

	provideCellStatusBarItems(
		cell: vscode.NotebookCell,
	): vscode.ProviderResult<vscode.NotebookCellStatusBarItem[]> {
		const meta = cellMetadata(cell);
		if (!meta?.cmType) {
			return [];
		}
		const item = new vscode.NotebookCellStatusBarItem(
			cellStatusText(meta),
			vscode.NotebookCellStatusBarAlignment.Left,
		);
		item.tooltip = cellStatusTooltip(meta);
		const result = this.results.get(cell.document.uri.toString());
		return result ? [item, resultStatusItem(result)] : [item];
	}

	setResult(uri: vscode.Uri, violations: CheckReport["files"][number]["violations"]): void {
		this.results.set(uri.toString(), cellResultStatus(violations));
		this.changed.fire();
	}

	clearNotebook(notebook: vscode.NotebookDocument): void {
		let changed = false;
		for (const cell of notebook.getCells()) {
			changed = this.results.delete(cell.document.uri.toString()) || changed;
		}
		if (changed) {
			this.changed.fire();
		}
	}

	dispose(): void {
		this.changed.dispose();
	}
}

interface CellResultStatus {
	errors: number;
	warnings: number;
	violations: CheckReport["files"][number]["violations"];
}

function cellResultStatus(
	violations: CheckReport["files"][number]["violations"],
): CellResultStatus {
	return { ...severityCounts(violations), violations };
}

function resultStatusItem(result: CellResultStatus): vscode.NotebookCellStatusBarItem {
	const item = new vscode.NotebookCellStatusBarItem(
		resultStatusText(result),
		vscode.NotebookCellStatusBarAlignment.Right,
	);
	item.tooltip = resultStatusTooltip(result);
	if (result.errors > 0) {
		setStatusBarColor(item, "testing.iconFailed");
	} else if (result.warnings > 0) {
		setStatusBarColor(item, "editorWarning.foreground");
	} else {
		setStatusBarColor(item, "testing.iconPassed");
	}
	return item;
}

function setStatusBarColor(
	item: vscode.NotebookCellStatusBarItem,
	color: string,
): void {
	(item as vscode.NotebookCellStatusBarItem & { color?: vscode.ThemeColor }).color =
		new vscode.ThemeColor(color);
}

function resultStatusText(result: CellResultStatus): string {
	if (result.errors > 0) {
		return `$(error) ${result.errors}`;
	}
	if (result.warnings > 0) {
		return `$(warning) ${result.warnings}`;
	}
	return "$(pass) clean";
}

function resultStatusTooltip(result: CellResultStatus): string {
	if (result.violations.length === 0) {
		return "No code-moniker violations";
	}
	return result.violations
		.map((violation) => `${violation.rule_id} ${lineRangeLabel(violation.lines)}: ${violation.explanation ?? violation.message}`)
		.join("\n");
}

function cellStatusText(meta: Partial<ScenarioCellMeta>): string {
	if (meta.cmType === "rules") {
		return "$(settings) rules";
	}
	return `$(file-code) ${meta.path ?? "file"}`;
}

function cellStatusTooltip(meta: Partial<ScenarioCellMeta>): string {
	if (meta.cmType === "rules") {
		return "Workspace check rules";
	}
	return meta.path ?? "Scenario file";
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

function fileCell(
	notebook: vscode.NotebookDocument,
	path: string,
): vscode.NotebookCell | undefined {
	const index = fileCellIndex(notebook, path);
	return index === undefined ? undefined : notebook.cellAt(index);
}

function fileCellIndex(
	notebook: vscode.NotebookDocument,
	path: string,
): number | undefined {
	for (let index = 0; index < notebook.cellCount; index++) {
		const cell = notebook.cellAt(index);
		const meta = cell.metadata as Partial<ScenarioCellMeta> | undefined;
		if (meta?.cmType === "file" && scenarioPathKey(meta.path) === scenarioPathKey(path)) {
			return index;
		}
	}
	return undefined;
}

function ruleCellIndex(
	notebook: vscode.NotebookDocument,
	ruleId: string,
): number | undefined {
	const localRuleId = ruleId.split(".").at(-1) ?? ruleId;
	for (let index = 0; index < notebook.cellCount; index++) {
		const cell = notebook.cellAt(index);
		const meta = cell.metadata as Partial<ScenarioCellMeta> | undefined;
		if (meta?.cmType !== "rules") {
			continue;
		}
		const text = cell.document.getText();
		if (text.includes(ruleId) || text.includes(`"${localRuleId}"`)) {
			return index;
		}
	}
	return firstRulesCellIndex(notebook);
}

function firstRulesCellIndex(notebook: vscode.NotebookDocument): number | undefined {
	for (let index = 0; index < notebook.cellCount; index++) {
		const meta = notebook.cellAt(index).metadata as Partial<ScenarioCellMeta> | undefined;
		if (meta?.cmType === "rules") {
			return index;
		}
	}
	return undefined;
}

async function revealNotebookCell(
	editor: vscode.NotebookEditor,
	index: number,
): Promise<void> {
	const range = new vscode.NotebookRange(index, index + 1);
	const shown = await vscode.window.showNotebookDocument(editor.notebook, {
		viewColumn: editor.viewColumn,
		selections: [range],
		preserveFocus: false,
	});
	shown.selection = range;
	shown.selections = [range];
	shown.revealRange(range, vscode.NotebookEditorRevealType.InCenterIfOutsideViewport);
}

function activeScenarioEditor(): vscode.NotebookEditor | undefined {
	const editor = vscode.window.activeNotebookEditor;
	return editor?.notebook.notebookType === SCENARIO_NOTEBOOK_TYPE ? editor : undefined;
}

async function revealCellLine(
	editor: vscode.NotebookEditor,
	cell: vscode.NotebookCell,
	line: number,
	output: vscode.OutputChannel,
): Promise<void> {
	const documentLine = Math.max(0, Math.min(line - 1, cell.document.lineCount - 1));
	const position = new vscode.Position(documentLine, 0);
	const range = new vscode.Range(position, position);
	try {
		await vscode.commands.executeCommand("notebook.cell.edit");
		await nextTick();
		const textEditor = vscode.window.activeTextEditor;
		if (textEditor?.document.uri.toString() !== cell.document.uri.toString()) {
			output.appendLine(
				`Could not focus scenario cell editor for ${scenarioPathFromCell(cell)} line ${line}.`,
			);
			return;
		}
		textEditor.selection = new vscode.Selection(position, position);
		textEditor.revealRange(range, vscode.TextEditorRevealType.InCenterIfOutsideViewport);
	} catch (error) {
		output.appendLine(
			`Could not reveal ${scenarioPathFromCell(cell)} line ${line}: ${errorMessage(error)}`,
		);
	}
}

function scenarioPathKey(path: string | undefined): string {
	return (path ?? "")
		.replace(/\\/g, "/")
		.replace(/^\.\/+/, "")
		.replace(/\/+/g, "/");
}

function scenarioPathFromCell(cell: vscode.NotebookCell): string {
	const meta = cell.metadata as Partial<ScenarioCellMeta> | undefined;
	return meta?.path ?? cell.document.uri.toString();
}

function errorMessage(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function nextTick(): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, 0));
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

function checkOutput(report: CheckReport, target: string): vscode.NotebookCellOutput {
	const payload: CheckOutputPayload = {
		kind: "check",
		target,
		summary: report.summary,
		files: report.files,
		...(report.errors ? { errors: report.errors } : {}),
	};
	return new vscode.NotebookCellOutput([
		vscode.NotebookCellOutputItem.json(payload, CHECK_OUTPUT_MIME),
		vscode.NotebookCellOutputItem.text(checkOutputText(report, target), "text/plain"),
	]);
}

async function replaceOutput(
	outputCell: vscode.NotebookCell,
	reportOrText: CheckReport | string,
	target: string | undefined,
	executions: vscode.NotebookCellExecution[],
): Promise<void> {
	const output = typeof reportOrText === "string"
		? textOutput(reportOrText)
		: checkOutput(reportOrText, target ?? ".");
	await Promise.all(
		executions.map((execution) =>
			execution.cell === outputCell
				? execution.replaceOutput(output)
				: execution.clearOutput(),
		),
	);
}

async function replaceOutputsDirect(
	notebook: vscode.NotebookDocument,
	cells: vscode.NotebookCell[],
	outputCell: vscode.NotebookCell | undefined,
	output: vscode.NotebookCellOutput | undefined,
): Promise<void> {
	const edit = new vscode.WorkspaceEdit();
	const notebookEdits: vscode.NotebookEdit[] = [];
	for (const cell of cells.filter((cell) => cell.kind === vscode.NotebookCellKind.Code)) {
		notebookEdits.push(
			vscode.NotebookEdit.replaceCells(
				new vscode.NotebookRange(cell.index, cell.index + 1),
				[cellDataFromCell(cell, cell === outputCell && output ? [output] : [])],
			),
		);
	}
	edit.set(notebook.uri, notebookEdits);
	await vscode.workspace.applyEdit(edit);
}

function cellDataFromCell(
	cell: vscode.NotebookCell,
	outputs: vscode.NotebookCellOutput[],
): vscode.NotebookCellData {
	const data = new vscode.NotebookCellData(
		cell.kind,
		cell.document.getText(),
		cell.document.languageId,
	);
	data.metadata = cell.metadata;
	data.outputs = outputs;
	return data;
}

function endAll(executions: vscode.NotebookCellExecution[], success: boolean): void {
	const endedAt = Date.now();
	for (const execution of executions) {
		execution.end(success, endedAt);
	}
}

function outputCellFor(cells: vscode.NotebookCell[]): vscode.NotebookCell {
	return cells.find((cell) => cellMetadata(cell)?.cmType === "rules")
		?? cells.find((cell) => cellMetadata(cell)?.cmType === "file")
		?? cells[0];
}

function runnableCells(cells: vscode.NotebookCell[]): vscode.NotebookCell[] {
	return cells.filter((cell) => {
		const meta = cellMetadata(cell);
		return cell.kind === vscode.NotebookCellKind.Code && Boolean(meta?.cmType);
	});
}

function targetFilePath(cell: vscode.NotebookCell): string | undefined {
	const meta = cellMetadata(cell);
	return meta?.cmType === "file" ? meta.path : undefined;
}

function cellMetadata(cell: vscode.NotebookCell): Partial<ScenarioCellMeta> | undefined {
	return cell.metadata as Partial<ScenarioCellMeta> | undefined;
}

function checkSucceeded(report: CheckReport): boolean {
	return report.summary.total_errors === 0
		&& (report.summary.total_rule_errors ?? report.summary.total_violations) === 0;
}

function checkOutputText(report: CheckReport, target: string): string {
	const lines = [
		`code-moniker check ${target}`,
		`${report.summary.total_violations} violation(s), ${report.summary.total_errors} error(s), ${report.summary.total_warnings} warning(s) across ${report.summary.files_scanned} file(s).`,
	];
	for (const file of report.files) {
		for (const group of groupVisibleViolations(file.violations)) {
			const violation = group.violation;
			const range = lineRangeLabel(violation.lines);
			const message = violation.explanation ?? violation.message;
			lines.push(
				`${file.file}:${range} [${violation.rule_id}] ${visibleViolationDetail(group)} - ${message}`,
			);
		}
	}
	for (const error of report.errors ?? []) {
		lines.push(`${error.file}: ${error.error}`);
	}
	return `${lines.join("\n")}\n`;
}

function isCheckOutputRendererMessage(
	value: unknown,
): value is RendererMessage {
	if (!value || typeof value !== "object") {
		return false;
	}
	const message = value as Partial<RendererMessage>;
	if (message.command === "revealRule") {
		return typeof message.ruleId === "string";
	}
	if (message.command === "revealFile") {
		return typeof message.file === "string";
	}
	return message.command === "revealLine"
		&& typeof message.file === "string"
		&& typeof message.line === "number";
}
