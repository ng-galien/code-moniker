import * as vscode from "vscode";

import { parseScenarioMarkdown } from "./markdown";
import { SCENARIO_NOTEBOOK_TYPE } from "./model";

export interface ScenarioDocumentOpenOptions {
	id: string;
	fileName: string;
	storageUri: vscode.Uri;
}

// Opens a catalog scenario as a generated clean notebook file. The builtin
// document stays bundled; the opened clone starts clean and only becomes
// save-worthy after the user edits it.
export async function openScenarioDocument(
	document: string,
	options: ScenarioDocumentOpenOptions,
): Promise<void> {
	const uri = await writeCleanScenarioClone(document, options);
	await openScenarioFile(uri);
}

// Opens an on-disk scenario file as a file-backed scenario notebook, forcing
// the scenario notebook editor.
export async function openScenarioFile(uri: vscode.Uri): Promise<void> {
	await vscode.commands.executeCommand(
		"vscode.openWith",
		uri,
		SCENARIO_NOTEBOOK_TYPE,
	);
}

async function writeCleanScenarioClone(
	document: string,
	options: ScenarioDocumentOpenOptions,
): Promise<vscode.Uri> {
	parseScenarioMarkdown(document);
	const folder = vscode.Uri.joinPath(options.storageUri, "catalog-samples");
	await vscode.workspace.fs.createDirectory(folder);
	const uri = await nextScenarioCloneUri(folder, options.fileName);
	await vscode.workspace.fs.writeFile(uri, new TextEncoder().encode(document));
	return uri;
}

async function nextScenarioCloneUri(
	folder: vscode.Uri,
	fileName: string,
): Promise<vscode.Uri> {
	const name = scenarioFileName(fileName);
	const stem = name.slice(0, -".cm.md".length);
	for (let index = 0; ; index += 1) {
		const candidate =
			index === 0 ? name : `${stem}-${index + 1}.cm.md`;
		const uri = vscode.Uri.joinPath(folder, candidate);
		if (!await exists(uri)) {
			return uri;
		}
	}
}

async function exists(uri: vscode.Uri): Promise<boolean> {
	try {
		await vscode.workspace.fs.stat(uri);
		return true;
	} catch {
		return false;
	}
}

function scenarioFileName(fileName: string): string {
	const leaf = (fileName.split(/[\\/]/).pop() ?? "").trim();
	const safe = (leaf || "scenario").replace(/[^A-Za-z0-9._-]+/g, "-");
	if (safe.endsWith(".cm.md")) {
		return safe;
	}
	if (safe.endsWith(".md")) {
		return `${safe.slice(0, -".md".length)}.cm.md`;
	}
	return `${safe}.cm.md`;
}
