import * as vscode from "vscode";

import { ScenarioController } from "./controller";
import { ScenarioSerializer } from "./serializer";
import { openScenarioDocument } from "./open";
import { SCENARIO_NOTEBOOK_TYPE } from "./model";
import { loadPackIndex, loadPackScenario } from "../catalog/packs";

export function registerScenario(context: vscode.ExtensionContext): void {
	context.subscriptions.push(
		vscode.workspace.registerNotebookSerializer(
			SCENARIO_NOTEBOOK_TYPE,
			new ScenarioSerializer(),
		),
		new ScenarioController(),
		vscode.commands.registerCommand(
			"codeMoniker.openSampleScenario",
			openSampleScenario,
		),
	);
}

// Pick a published sample from the CLI and open it as a scenario notebook.
async function openSampleScenario(): Promise<void> {
	const index = await loadPackIndex();
	if (!index.ok) {
		void vscode.window.showErrorMessage(index.error);
		return;
	}
	const pick = await vscode.window.showQuickPick(
		index.packs.map((pack) => ({
			label: pack.name,
			description: pack.blurb,
			name: pack.name,
		})),
		{ title: "Open Sample Scenario", placeHolder: "Pick a sample" },
	);
	if (!pick) {
		return;
	}
	const scenario = await loadPackScenario(pick.name);
	if (!scenario.ok) {
		void vscode.window.showErrorMessage(scenario.error);
		return;
	}
	await openScenarioDocument(scenario.document);
}
