import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { SymbolTreeProvider } from "../symbols/tree";
import { ViolationModel } from "./decorations";
import { RulesRepository } from "./repository";
import { RulesProvider } from "./tree";

export function registerRulesDaemonCommands(
	context: vscode.ExtensionContext,
	session: DaemonSession,
	repository: RulesRepository,
	provider: RulesProvider,
	model: ViolationModel,
	symbolTree: SymbolTreeProvider,
): void {
	context.subscriptions.push(
		vscode.commands.registerCommand("codeMoniker.rulesDaemon.refresh", () =>
			refreshRules(session, provider),
		),
		vscode.commands.registerCommand("codeMoniker.rulesDaemon.runCheck", () =>
			runCheck(repository, provider, model, symbolTree),
		),
	);
}

async function refreshRules(
	session: DaemonSession,
	provider: RulesProvider,
): Promise<void> {
	if (!(await session.connectOrStart())) {
		return;
	}
	await session.refresh();
	provider.refresh();
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
	symbolTree.refresh();
	const total = result.summary.total_violations;
	void vscode.window.showInformationMessage(
		total === 0
			? `No violations across ${result.summary.files_scanned} file(s).`
			: `${total} violation(s) across ${result.summary.files_with_violations} file(s).`,
	);
}
