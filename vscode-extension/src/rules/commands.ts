import * as path from "node:path";
import * as vscode from "vscode";

import { runCheckProject, validateRuleFile } from "../cli/facade";
import { toDiagnostic } from "../diagnostics/vscode";
import { firstLine, rootOf, workspaceLabel } from "../shared/workspace";
import { RuleFileNode, RuleNode, RuleTreeNode } from "./nodes";
import { findRuleFiles, isFragmentFile, rulesEntrypoint } from "./repository";
import { RuleFilesProvider } from "./tree";

export function registerRuleCommands(
	context: vscode.ExtensionContext,
	provider: RuleFilesProvider,
	treeView: vscode.TreeView<RuleTreeNode>,
	diagnostics: vscode.DiagnosticCollection,
): void {
	context.subscriptions.push(
		vscode.commands.registerCommand("codeMoniker.refreshRuleFiles", () => provider.refresh()),
		vscode.commands.registerCommand("codeMoniker.expandRuleFiles", () =>
			expandRuleFiles(provider, treeView),
		),
		vscode.commands.registerCommand("codeMoniker.collapseRuleFiles", () =>
			vscode.commands.executeCommand("workbench.actions.treeView.codeMoniker.ruleFiles.collapseAll"),
		),
		vscode.commands.registerCommand("codeMoniker.openRuleFile", (node: RuleFileNode) =>
			openRuleFile(node),
		),
		vscode.commands.registerCommand("codeMoniker.copyRuleFileRelativePath", (node: RuleFileNode) =>
			copyRelativePath(node),
		),
		vscode.commands.registerCommand("codeMoniker.revealRule", (node: RuleNode) =>
			revealPickedRule(node),
		),
		vscode.commands.registerCommand("codeMoniker.validateRuleFile", (node: RuleFileNode) =>
			validatePickedFile(node, diagnostics, provider),
		),
		vscode.commands.registerCommand("codeMoniker.runRuleFileOnProject", (node: RuleFileNode) =>
			runPickedFile(node, diagnostics, provider),
		),
	);
}

async function expandRuleFiles(
	provider: RuleFilesProvider,
	treeView: vscode.TreeView<RuleTreeNode>,
): Promise<void> {
	for (const node of await provider.getChildren()) {
		await expandNode(provider, treeView, node);
	}
}

async function expandNode(
	provider: RuleFilesProvider,
	treeView: vscode.TreeView<RuleTreeNode>,
	node: RuleTreeNode,
): Promise<void> {
	if (node.kind !== "folder" && node.kind !== "file") {
		return;
	}
	await treeView.reveal(node, { expand: true });
	for (const child of await provider.getChildren(node)) {
		await expandNode(provider, treeView, child);
	}
}

async function copyRelativePath(node: RuleFileNode | undefined): Promise<void> {
	const target = node ?? await pickRuleFile("Copy rule file relative path");
	if (!target) {
		return;
	}
	const relativePath = workspaceLabel(target.uri);
	await vscode.env.clipboard.writeText(relativePath);
	void vscode.window.showInformationMessage(`Copied ${relativePath}`);
}

async function openRuleFile(node: RuleFileNode | undefined): Promise<void> {
	const target = node ?? await pickRuleFile("Open rule file");
	if (target) {
		await vscode.window.showTextDocument(target.uri);
	}
}

async function revealPickedRule(node: RuleNode | undefined): Promise<void> {
	const target = node ?? await pickRule("Reveal rule");
	if (target) {
		await revealRule(target);
	}
}

async function revealRule(node: RuleNode): Promise<void> {
	const doc = await vscode.workspace.openTextDocument(node.uri);
	const editor = await vscode.window.showTextDocument(doc);
	const pos = new vscode.Position(node.rule.line, 0);
	editor.selection = new vscode.Selection(pos, pos);
	editor.revealRange(new vscode.Range(pos, pos), vscode.TextEditorRevealType.InCenter);
}

async function validatePickedFile(
	node: RuleFileNode | undefined,
	diagnostics: vscode.DiagnosticCollection,
	provider?: RuleFilesProvider,
): Promise<void> {
	const target = node ?? await pickRuleFile("Validate rules");
	if (target) {
		await validate(target, diagnostics, provider);
	}
}

async function validate(
	node: RuleFileNode,
	diagnostics: vscode.DiagnosticCollection,
	provider?: RuleFilesProvider,
): Promise<void> {
	const result = await vscode.window.withProgress(
		{ location: vscode.ProgressLocation.Window, title: "Validating rules…" },
		async () => {
			const entrypoint = rulesEntrypoint(node.uri, workspaceLabel(node.uri));
			return entrypoint.ok
				? validateRuleFile(rootOf(node.uri), entrypoint.uri.fsPath)
				: entrypoint;
		},
	);
	if (result.ok) {
		diagnostics.delete(node.uri);
		provider?.markValidation(node.uri, "clean");
		void vscode.window.showInformationMessage(
			`${workspaceLabel(node.uri)}: rules compile cleanly${entrypointSuffix(node)}.`,
		);
		return;
	}
	provider?.markValidation(node.uri, "failed");
	diagnostics.set(node.uri, [
		new vscode.Diagnostic(
			new vscode.Range(0, 0, 0, 1),
			result.error,
			vscode.DiagnosticSeverity.Error,
		),
	]);
	void vscode.window.showErrorMessage(`${workspaceLabel(node.uri)}: ${firstLine(result.error)}`);
}

async function runPickedFile(
	node: RuleFileNode | undefined,
	diagnostics: vscode.DiagnosticCollection,
	provider?: RuleFilesProvider,
): Promise<void> {
	const target = node ?? await pickRuleFile("Run rules on project");
	if (target) {
		await runOnProject(target, diagnostics, provider);
	}
}

async function runOnProject(
	node: RuleFileNode,
	diagnostics: vscode.DiagnosticCollection,
	provider?: RuleFilesProvider,
): Promise<void> {
	const root = rootOf(node.uri);
	const result = await vscode.window.withProgress(
		{ location: vscode.ProgressLocation.Notification, title: "code-moniker check…" },
		async () => {
			const entrypoint = rulesEntrypoint(node.uri, workspaceLabel(node.uri));
			return entrypoint.ok
				? runCheckProject(root, entrypoint.uri.fsPath)
				: entrypoint;
		},
	);
	if (!result.ok) {
		provider?.markRunFailed(node.uri);
		void vscode.window.showErrorMessage(firstLine(result.error));
		return;
	}
	if (provider) {
		provider.markRunForFiles(await findRuleFiles(), result.report);
	}
	diagnostics.clear();
	let total = 0;
	for (const file of result.report.files) {
		const uri = vscode.Uri.file(path.resolve(root, file.file));
		diagnostics.set(uri, file.violations.map(toDiagnostic));
		total += file.violations.length;
	}
	const message =
		total === 0
			? `${workspaceLabel(node.uri)}: no violations across ${result.report.summary.files_scanned} file(s).`
			: `${workspaceLabel(node.uri)}: ${total} violation(s). See the Problems panel.`;
	void vscode.window.showInformationMessage(message);
	if (total > 0) {
		void vscode.commands.executeCommand("workbench.panel.markers.view.focus");
	}
}

function entrypointSuffix(node: RuleFileNode): string {
	if (!isFragmentFile(node.uri)) {
		return "";
	}
	const entrypoint = rulesEntrypoint(node.uri, workspaceLabel(node.uri));
	return entrypoint.ok ? ` via ${workspaceLabel(entrypoint.uri)}` : "";
}

async function pickRuleFile(title: string): Promise<RuleFileNode | undefined> {
	const files = await findRuleFiles();
	if (files.length === 0) {
		void vscode.window.showWarningMessage("No Code Moniker rule file found.");
		return undefined;
	}
	const pick = await vscode.window.showQuickPick(
		files.map((file) => ({
			label: workspaceLabel(file.uri),
			description: `${file.parsed.rules.length} rule(s)`,
			file,
		})),
		{ title },
	);
	return pick?.file;
}

async function pickRule(title: string): Promise<RuleNode | undefined> {
	const files = await findRuleFiles();
	const rules = files.flatMap((file) =>
		file.parsed.rules.map((rule) => ({
			label: rule.id,
			description: rule.scope,
			detail: workspaceLabel(file.uri),
			node: { kind: "rule" as const, uri: file.uri, rule },
		})),
	);
	if (rules.length === 0) {
		void vscode.window.showWarningMessage("No Code Moniker rule found.");
		return undefined;
	}
	const pick = await vscode.window.showQuickPick(rules, { title });
	return pick?.node;
}
