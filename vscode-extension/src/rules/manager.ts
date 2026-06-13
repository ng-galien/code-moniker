import * as vscode from "vscode";

import { registerRuleCommands } from "./commands";
import { RULE_GLOB } from "./repository";
import { RuleFilesProvider } from "./tree";

export function registerRuleManager(context: vscode.ExtensionContext): void {
	const provider = new RuleFilesProvider();
	const diagnostics = vscode.languages.createDiagnosticCollection("code-moniker");
	const watcher = vscode.workspace.createFileSystemWatcher(RULE_GLOB);
	const treeView = vscode.window.createTreeView("codeMoniker.ruleFiles", {
		treeDataProvider: provider,
		showCollapseAll: false,
	});

	context.subscriptions.push(
		diagnostics,
		treeView,
		watcher,
		watcher.onDidCreate(() => provider.refresh()),
		watcher.onDidDelete(() => provider.refresh()),
		watcher.onDidChange(() => provider.refresh()),
	);

	registerRuleCommands(context, provider, treeView, diagnostics);
}
