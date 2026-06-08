import * as vscode from "vscode";

import { LangDef, langById } from "../shared/languages";
import { RuleCellMeta, SampleCellMeta } from "./model";

export interface RuleCellContext {
	cell: vscode.NotebookCell;
	lang: LangDef;
	language: string;
	rulesToml: string;
}

export interface SampleCellContext {
	cell: vscode.NotebookCell;
	lang: LangDef;
	language: string;
}

export function sampleContext(
	cell: vscode.NotebookCell,
): SampleCellContext | undefined {
	const meta = cell.metadata as Partial<SampleCellMeta> | undefined;
	if (meta?.cmType !== "sample") {
		return undefined;
	}
	const language = meta.language ?? "";
	const lang = langById(language);
	return lang ? { cell, lang, language } : undefined;
}

export function ruleContext(
	cell: vscode.NotebookCell,
): RuleCellContext | undefined {
	const meta = cell.metadata as Partial<RuleCellMeta> | undefined;
	if (meta?.cmType !== "rule") {
		return undefined;
	}
	const language = meta.language ?? "";
	const lang = langById(language);
	return lang
		? { cell, lang, language, rulesToml: cell.document.getText().trim() }
		: undefined;
}

export function followingRuleCells(
	sample: SampleCellContext,
): RuleCellContext[] {
	const rules: RuleCellContext[] = [];
	const notebook = sample.cell.notebook;
	for (let index = sample.cell.index + 1; index < notebook.cellCount; index++) {
		const candidate = notebook.cellAt(index);
		if (sampleContext(candidate)) {
			break;
		}
		const rule = ruleContext(candidate);
		if (rule?.language === sample.language && rule.rulesToml) {
			rules.push(rule);
		}
	}
	return rules;
}

export function selectedRuleCell(): vscode.NotebookCell | undefined {
	const editor = vscode.window.activeNotebookEditor;
	if (!editor) {
		return undefined;
	}
	const cell = editor.notebook.cellAt(editor.selection.start);
	return ruleContext(cell) ? cell : undefined;
}
