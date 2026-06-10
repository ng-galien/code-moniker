import { registerCatalog } from "./catalog/catalogView";
import { registerNotebook } from "./notebook/manager";
import { registerRuleManager } from "./rules/manager";
import { registerScenario } from "./scenario/manager";
import * as vscode from "vscode";

export function activate(context: vscode.ExtensionContext): void {
	registerNotebook(context);
	registerRuleManager(context);
	registerCatalog(context);
	registerScenario(context);
}

export function deactivate(): void {
	// Controllers and serializers are disposed via context.subscriptions.
}
