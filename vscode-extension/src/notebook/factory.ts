import * as vscode from "vscode";

import { langById } from "../shared/languages";
import { CmnbCell } from "./model";
import { sampleText } from "./samples";
import { toCellData } from "./serializer";

const NOTEBOOK_TYPE = "code-moniker";

// Opens an in-memory .cmnb notebook from a list of cells.
export async function openNotebook(cells: CmnbCell[]): Promise<void> {
	const data = new vscode.NotebookData(cells.map(toCellData));
	const notebook = await vscode.workspace.openNotebookDocument(NOTEBOOK_TYPE, data);
	await vscode.window.showNotebookDocument(notebook);
}

// The standard lesson layout: an explanation, a sample, and a rule to run on it.
export function lessonCells(
	title: string,
	blurb: string,
	langId: string,
	sample: string,
	ruleToml: string,
): CmnbCell[] {
	return [
		{ kind: "markdown", value: `# ${title}\n\n${blurb}` },
		{ kind: "sample", language: langId, value: sample },
		{ kind: "rule", language: langId, value: ruleToml },
	];
}

// A scratch notebook to test a single rule fragment against a sample.
export async function openScratchNotebook(
	title: string,
	langId: string,
	ruleToml: string,
): Promise<void> {
	const label = langById(langId)?.label ?? langId;
	const blurb =
		`Edit the ${label} **sample** below or the **rule** TOML, then run the rule cell (▷) ` +
		"to see the violations. This is a scratch notebook — nothing is saved to your project.";
	await openNotebook(lessonCells(`Test: ${title}`, blurb, langId, sampleText(langId), ruleToml));
}
