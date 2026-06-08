import * as vscode from "vscode";

import { addRuleCell, addSampleCell, copyRuleToProjectConfig, newNotebook } from "./commands";
import { CmnbController } from "./controller";
import { CmnbSerializer } from "./serializer";
import { CmnbCellStatusBarProvider } from "./statusBar";

export function registerNotebook(context: vscode.ExtensionContext): void {
	context.subscriptions.push(
		vscode.workspace.registerNotebookSerializer(
			"code-moniker",
			new CmnbSerializer(),
			{ transientOutputs: true },
		),
		new CmnbController(context.extensionUri.fsPath),
		vscode.notebooks.registerNotebookCellStatusBarItemProvider(
			"code-moniker",
			new CmnbCellStatusBarProvider(),
		),
		vscode.commands.registerCommand("codeMoniker.newNotebook", newNotebook),
		vscode.commands.registerCommand("codeMoniker.addSampleCell", addSampleCell),
		vscode.commands.registerCommand("codeMoniker.addRuleCell", addRuleCell),
		vscode.commands.registerCommand(
			"codeMoniker.copyRuleToProjectConfig",
			copyRuleToProjectConfig,
		),
	);
}

