import * as vscode from "vscode";

import { registerRuleCommands } from "./commands";
import { RULE_GLOB } from "./repository";
import { RuleFilesProvider } from "./tree";

export interface RuleFilesFeature {
	provider: RuleFilesProvider;
}

export function registerRuleManager(context: vscode.ExtensionContext): RuleFilesFeature {
	const provider = new RuleFilesProvider();
	const diagnostics = vscode.languages.createDiagnosticCollection("code-moniker");
	const watcher = vscode.workspace.createFileSystemWatcher(RULE_GLOB);

	context.subscriptions.push(
		diagnostics,
		watcher,
		watcher.onDidCreate(() => provider.refresh()),
		watcher.onDidDelete(() => provider.refresh()),
		watcher.onDidChange(() => provider.refresh()),
	);

	registerRuleCommands(context, provider, undefined, diagnostics);
	return { provider };
}
