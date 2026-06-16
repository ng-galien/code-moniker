import * as vscode from "vscode";

import { ViolationModel } from "./decorations";
import { RulesRepository } from "./repository";
import { RulesProvider } from "./tree";
import { SymbolTreeProvider } from "../symbols/tree";

export function registerRulesDaemonCommands(
	context: vscode.ExtensionContext,
	repository: RulesRepository,
	provider: RulesProvider,
	model: ViolationModel,
	symbolTree: SymbolTreeProvider,
): void {
	context.subscriptions.push(
		vscode.commands.registerCommand("codeMoniker.rulesDaemon.refresh", () => provider.refresh()),
		vscode.commands.registerCommand("codeMoniker.rulesDaemon.runCheck", () =>
			runCheck(repository, provider, model, symbolTree),
		),
	);
}

async function runCheck(
	repository: RulesRepository,
	provider: RulesProvider,
	model: ViolationModel,
	symbolTree: SymbolTreeProvider,
): Promise<void> {
	if (!repository.ready) {
		void vscode.window.showWarningMessage("Daemon is not ready yet.");
		return;
	}
	const result = await vscode.window.withProgress(
		{ location: vscode.ProgressLocation.Notification, title: "code-moniker check…" },
		() => repository.runCheck(),
	);
	provider.setCheck(result.summary, result.violations);
	model.update(result.violations);
	// The symbol tree already holds this model (registered once); just re-render.
	symbolTree.refresh();
	const total = result.summary.total_violations;
	void vscode.window.showInformationMessage(
		total === 0
			? `No violations across ${result.summary.files_scanned} file(s).`
			: `${total} violation(s) across ${result.summary.files_with_violations} file(s).`,
	);
}
