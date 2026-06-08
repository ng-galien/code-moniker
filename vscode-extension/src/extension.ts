import { registerCatalog } from "./catalog/catalogView";
import { registerNotebook } from "./notebook/manager";
import { registerRuleManager } from "./rules/manager";
import * as vscode from "vscode";

export function activate(context: vscode.ExtensionContext): void {
	registerNotebook(context);
	registerRuleManager(context);
	registerCatalog(context);
}

export function deactivate(): void {
	// Controllers and serializers are disposed via context.subscriptions.
}
