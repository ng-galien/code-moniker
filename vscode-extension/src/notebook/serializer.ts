import * as vscode from "vscode";

import { langById } from "../shared/languages";
import { CmnbCell, CmnbDocument, RuleCellMeta, SampleCellMeta } from "./model";

// Rule cells are authored as real .code-moniker.toml; this language id wires up
// TOML highlighting plus the embedded check-DSL grammar (see syntaxes/).
const RULE_LANGUAGE_ID = "cmrule-toml";

interface AnyCellMeta {
	cmType?: "rule" | "sample";
	language?: string;
}

interface NotebookMeta {
	title?: string;
	catalog?: CmnbDocument["catalog"];
}

// Converts .cmnb JSON <-> VSCode NotebookData.
export class CmnbSerializer implements vscode.NotebookSerializer {
	deserializeNotebook(content: Uint8Array): vscode.NotebookData {
		const text = new TextDecoder().decode(content).trim();
		const doc: CmnbDocument = text.length
			? JSON.parse(text)
			: { version: 1, cells: [] };
		const cells = (doc.cells ?? []).map(toCellData);
		const data = new vscode.NotebookData(cells);
		data.metadata = notebookMetadata(doc);
		return data;
	}

	serializeNotebook(data: vscode.NotebookData): Uint8Array {
		const meta = data.metadata as NotebookMeta | undefined;
		const doc: CmnbDocument = {
			version: 1,
			...documentMetadata(meta),
			cells: data.cells.map(fromCellData),
		};
		const json = JSON.stringify(doc, null, "\t") + "\n";
		return new TextEncoder().encode(json);
	}
}

export function toCellData(cell: CmnbCell): vscode.NotebookCellData {
	if (cell.kind === "markdown") {
		return new vscode.NotebookCellData(
			vscode.NotebookCellKind.Markup,
			cell.value,
			"markdown",
		);
	}
	if (cell.kind === "sample") {
		const lang = langById(cell.language);
		const data = new vscode.NotebookCellData(
			vscode.NotebookCellKind.Code,
			cell.value,
			lang ? lang.vscodeId : "plaintext",
		);
		const meta: SampleCellMeta = { cmType: "sample", language: cell.language };
		data.metadata = meta;
		return data;
	}
	// rule: the body is a real .code-moniker.toml fragment.
	const data = new vscode.NotebookCellData(
		vscode.NotebookCellKind.Code,
		cell.value,
		RULE_LANGUAGE_ID,
	);
	const meta: RuleCellMeta = { cmType: "rule", language: cell.language };
	data.metadata = meta;
	return data;
}

function fromCellData(cell: vscode.NotebookCellData): CmnbCell {
	if (cell.kind === vscode.NotebookCellKind.Markup) {
		return { kind: "markdown", value: cell.value };
	}
	const meta = cell.metadata as AnyCellMeta | undefined;
	if (meta?.cmType === "rule") {
		return {
			kind: "rule",
			language: meta.language ?? "rust",
			value: cell.value,
		};
	}
	// default: any other code cell is a sample, recovering the language.
	const language = meta?.language ?? cell.languageId;
	return { kind: "sample", language, value: cell.value };
}

function notebookMetadata(doc: CmnbDocument): NotebookMeta {
	return {
		...(doc.title ? { title: doc.title } : {}),
		...(doc.catalog ? { catalog: doc.catalog } : {}),
	};
}

function documentMetadata(meta: NotebookMeta | undefined): NotebookMeta {
	return {
		...(typeof meta?.title === "string" ? { title: meta.title } : {}),
		...(meta?.catalog ? { catalog: meta.catalog } : {}),
	};
}
