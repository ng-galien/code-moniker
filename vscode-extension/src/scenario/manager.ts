import * as vscode from "vscode";

import { ScenarioController } from "./controller";
import { ScenarioSerializer } from "./serializer";
import { openScenarioDocument } from "./open";
import { SCENARIO_NOTEBOOK_TYPE } from "./model";
import { loadPackIndex } from "../catalog/packs";

export function registerScenario(context: vscode.ExtensionContext): void {
	context.subscriptions.push(
		vscode.workspace.registerNotebookSerializer(
			SCENARIO_NOTEBOOK_TYPE,
			new ScenarioSerializer(),
		),
		new ScenarioController(),
		vscode.commands.registerCommand(
			"codeMoniker.openSampleScenario",
			() => openSampleScenario(context.globalStorageUri),
		),
	);
}

// Pick a catalog sample and open it through the executable Markdown scenario view.
async function openSampleScenario(storageUri: vscode.Uri): Promise<void> {
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
	const document = index.packs.find((pack) => pack.name === pick.name)?.document;
	if (!document) {
		void vscode.window.showErrorMessage(`Unknown sample scenario \`${pick.name}\`.`);
		return;
	}
	await openScenarioDocument(document, {
		id: `builtin:pack:${pick.name}`,
		fileName: `${pick.name}.cm.md`,
		storageUri,
	});
}
