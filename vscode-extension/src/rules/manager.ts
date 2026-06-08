import * as vscode from "vscode";

import { registerRuleCommands } from "./commands";
import { RULE_GLOB } from "./repository";
import { RuleFilesProvider } from "./tree";

export function registerRuleManager(context: vscode.ExtensionContext): void {
	const provider = new RuleFilesProvider();
	const diagnostics = vscode.languages.createDiagnosticCollection("code-moniker");
	const watcher = vscode.workspace.createFileSystemWatcher(RULE_GLOB);

	context.subscriptions.push(
		diagnostics,
		vscode.window.registerTreeDataProvider("codeMoniker.ruleFiles", provider),
		watcher,
		watcher.onDidCreate(() => provider.refresh()),
		watcher.onDidDelete(() => provider.refresh()),
		watcher.onDidChange(() => provider.refresh()),
	);

	registerRuleCommands(context, provider, diagnostics);
}
