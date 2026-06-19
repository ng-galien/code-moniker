import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { SymbolFeature } from "../symbols/manager";
import { registerRulesDaemonCommands } from "./commands";
import { ViolationModel } from "./decorations";
import { RulesRepository } from "./repository";
import { RulesProvider } from "./tree";

// Adds the daemon-backed rules/check tree and overlays its violations onto the
// symbol tree, file badges, and the Problems panel.
export interface RulesFeature {
	provider: RulesProvider;
	model: ViolationModel;
}

export function registerRulesDaemon(
	context: vscode.ExtensionContext,
	session: DaemonSession,
	symbols: SymbolFeature,
): RulesFeature {
	const repository = new RulesRepository(session);
	const diagnostics = vscode.languages.createDiagnosticCollection("code-moniker-daemon");
	const model = new ViolationModel(diagnostics);
	const provider = new RulesProvider(session, repository);

	symbols.tree.setViolations(model);

	context.subscriptions.push(
		diagnostics,
		model,
		vscode.window.registerFileDecorationProvider(model),
		session.onDidChangeStatus(() => provider.refresh()),
	);

	registerRulesDaemonCommands(context, session, repository, provider, model, symbols.tree);

	return { provider, model };
}
