import * as vscode from "vscode";

import { parseScenarioMarkdown } from "./markdown";
import { scenarioToNotebook } from "./serializer";
import { SCENARIO_NOTEBOOK_TYPE } from "./model";

// Opens a scenario Markdown document as an (untitled) multi-file scenario
// notebook. Used by the catalog and the palette command so there is one path.
export async function openScenarioDocument(document: string): Promise<void> {
	const notebook = await vscode.workspace.openNotebookDocument(
		SCENARIO_NOTEBOOK_TYPE,
		scenarioToNotebook(parseScenarioMarkdown(document)),
	);
	await vscode.window.showNotebookDocument(notebook, { preview: false });
}

// Opens an on-disk scenario file as a file-backed scenario notebook, forcing
// the scenario notebook editor (the *.md selector is opt-in priority).
export async function openScenarioFile(uri: vscode.Uri): Promise<void> {
	await vscode.commands.executeCommand(
		"vscode.openWith",
		uri,
		SCENARIO_NOTEBOOK_TYPE,
	);
}
