import * as vscode from "vscode";

import { followingRuleCells, ruleContext, sampleContext } from "./cells";

export class CmnbCellStatusBarProvider implements vscode.NotebookCellStatusBarItemProvider {
	provideCellStatusBarItems(
		cell: vscode.NotebookCell,
		_token: vscode.CancellationToken,
	): vscode.ProviderResult<vscode.NotebookCellStatusBarItem[]> {
		const sample = sampleContext(cell);
		if (sample) {
			return sampleStatusItems(sample);
		}
		const rule = ruleContext(cell);
		if (rule) {
			return ruleStatusItems(rule.cell, rule.lang.label);
		}
		return [];
	}
}

function sampleStatusItems(
	sample: NonNullable<ReturnType<typeof sampleContext>>,
): vscode.NotebookCellStatusBarItem[] {
	const count = followingRuleCells(sample).length;
	const state = executionState(sample.cell, "clean", "violations");
	const text =
		count === 0
			? `$(warning) Sample · ${sample.lang.label} · no rules below`
			: `$(beaker) Sample · ${sample.lang.label} · checks ${count} rule(s)${state}`;
	const item = new vscode.NotebookCellStatusBarItem(
		text,
		vscode.NotebookCellStatusBarAlignment.Left,
	);
	item.tooltip =
		count === 0
			? "Run this cell after adding a rule cell below it."
			: "Run this sample cell to evaluate the following rule cells until the next sample.";
	item.priority = 100;
	return [item];
}

function ruleStatusItems(
	cell: vscode.NotebookCell,
	label: string,
): vscode.NotebookCellStatusBarItem[] {
	const state = executionState(cell, "valid", "invalid");
	const kind = new vscode.NotebookCellStatusBarItem(
		`$(law) Rule · ${label} · validates TOML${state}`,
		vscode.NotebookCellStatusBarAlignment.Left,
	);
	kind.tooltip = "Run this rule cell to validate the TOML fragment.";
	kind.priority = 100;

	const copy = new vscode.NotebookCellStatusBarItem(
		"$(copy) Copy to config",
		vscode.NotebookCellStatusBarAlignment.Right,
	);
	copy.command = {
		command: "codeMoniker.copyRuleToProjectConfig",
		title: "Copy to config",
		arguments: [cell],
	};
	copy.tooltip = "Append this rule fragment to the workspace .code-moniker.toml.";
	copy.priority = 10;
	return [kind, copy];
}

function executionState(
	cell: vscode.NotebookCell,
	successLabel: string,
	failureLabel: string,
): string {
	if (cell.executionSummary?.success === true) {
		return ` · ${successLabel}`;
	}
	if (cell.executionSummary?.success === false) {
		return ` · ${failureLabel}`;
	}
	return "";
}
