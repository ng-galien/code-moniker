import * as vscode from "vscode";

import { langByTomlSection } from "../shared/languages";
import { parseScenarioMarkdown, serializeScenarioMarkdown } from "./markdown";
import {
	ScenarioCell,
	ScenarioCellMeta,
	ScenarioDocument,
	ScenarioNotebookMeta,
} from "./model";

const RULE_LANGUAGE_ID = "cmrule-toml";

// Converts scenario Markdown <-> VSCode NotebookData (jupytext-style: the
// Markdown file is the notebook's storage format).
export class ScenarioSerializer implements vscode.NotebookSerializer {
	deserializeNotebook(content: Uint8Array): vscode.NotebookData {
		return scenarioToNotebook(
			parseScenarioMarkdown(new TextDecoder().decode(content)),
		);
	}

	serializeNotebook(data: vscode.NotebookData): Uint8Array {
		return new TextEncoder().encode(
			serializeScenarioMarkdown(notebookToScenario(data)),
		);
	}
}

export function scenarioToNotebook(document: ScenarioDocument): vscode.NotebookData {
	const data = new vscode.NotebookData(document.cells.map(toCellData));
	const meta: ScenarioNotebookMeta = {};
	if (document.frontMatter !== undefined) {
		meta.frontMatter = document.frontMatter;
	}
	data.metadata = meta;
	return data;
}

export function notebookToScenario(data: vscode.NotebookData): ScenarioDocument {
	const meta = data.metadata as ScenarioNotebookMeta | undefined;
	return {
		...(meta?.frontMatter !== undefined ? { frontMatter: meta.frontMatter } : {}),
		cells: data.cells.map(fromCellData),
	};
}

function toCellData(cell: ScenarioCell): vscode.NotebookCellData {
	if (cell.kind === "markup") {
		return new vscode.NotebookCellData(
			vscode.NotebookCellKind.Markup,
			cell.value,
			"markdown",
		);
	}
	const data = new vscode.NotebookCellData(
		vscode.NotebookCellKind.Code,
		cell.value.replace(/\n$/, ""),
		cellLanguage(cell),
	);
	data.metadata = cellMeta(cell);
	return data;
}

function cellLanguage(cell: ScenarioCell): string {
	if (cell.kind === "rules") {
		return RULE_LANGUAGE_ID;
	}
	if (cell.kind === "file") {
		return langByTomlSection(cell.fence)?.vscodeId ?? cell.fence ?? "plaintext";
	}
	return "plaintext";
}

function cellMeta(cell: ScenarioCell): ScenarioCellMeta {
	if (cell.kind === "file") {
		return { cmType: "file", path: cell.path, fence: cell.fence };
	}
	return { cmType: cell.kind === "rules" ? "rules" : "expect" };
}

function fromCellData(cell: vscode.NotebookCellData): ScenarioCell {
	if (cell.kind === vscode.NotebookCellKind.Markup) {
		return { kind: "markup", value: cell.value };
	}
	const value = cell.value.length ? `${cell.value}\n` : "";
	const meta = cell.metadata as Partial<ScenarioCellMeta> | undefined;
	if (meta?.cmType === "rules") {
		return { kind: "rules", value };
	}
	if (meta?.cmType === "expect") {
		return { kind: "expect", value };
	}
	return {
		kind: "file",
		path: meta?.path ?? "src/sample.txt",
		fence: meta?.fence ?? "",
		value,
	};
}
