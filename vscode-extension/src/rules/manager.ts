import * as vscode from "vscode";

import { registerRuleCommands } from "./commands";
import { watchAndRefresh } from "../shared/watch";
import { RULE_GLOB } from "./repository";
import { RuleFilesProvider } from "./tree";

export interface RuleFilesFeature {
	provider: RuleFilesProvider;
}

export function registerRuleManager(context: vscode.ExtensionContext): RuleFilesFeature {
	const provider = new RuleFilesProvider();
	const diagnostics = vscode.languages.createDiagnosticCollection("code-moniker");

	context.subscriptions.push(
		diagnostics,
		...watchAndRefresh(RULE_GLOB, () => provider.refresh()),
	);

	registerRuleCommands(context, provider, undefined, diagnostics);
	return { provider };
}
