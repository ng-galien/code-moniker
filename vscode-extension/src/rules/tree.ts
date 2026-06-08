import * as vscode from "vscode";

import { workspaceLabel } from "../shared/workspace";
import { RuleFileNode, RuleNode, RuleTreeNode } from "./nodes";
import { findRuleFiles } from "./repository";

export class RuleFilesProvider implements vscode.TreeDataProvider<RuleTreeNode> {
	private readonly emitter = new vscode.EventEmitter<RuleTreeNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;

	refresh(): void {
		this.emitter.fire(undefined);
	}

	async getChildren(node?: RuleTreeNode): Promise<RuleTreeNode[]> {
		if (!node) {
			const files = await findRuleFiles();
			if (files.length === 0) {
				return [{ kind: "info", label: "No .code-moniker.toml or *.fragment.toml found" }];
			}
			return files;
		}
		if (node.kind === "file") {
			if (node.parsed.rules.length === 0) {
				return [{ kind: "info", label: "No rules in this file" }];
			}
			return node.parsed.rules.map((rule) => ({ kind: "rule", uri: node.uri, rule }));
		}
		return [];
	}

	getTreeItem(node: RuleTreeNode): vscode.TreeItem {
		if (node.kind === "info") {
			return new vscode.TreeItem(node.label);
		}
		if (node.kind === "file") {
			return ruleFileTreeItem(node);
		}
		return ruleTreeItem(node);
	}
}

function ruleFileTreeItem(node: RuleFileNode): vscode.TreeItem {
	const item = new vscode.TreeItem(
		workspaceLabel(node.uri),
		vscode.TreeItemCollapsibleState.Expanded,
	);
	item.description = `${node.parsed.rules.length} rule(s)`;
	item.resourceUri = node.uri;
	item.iconPath = new vscode.ThemeIcon("law");
	item.contextValue = "cmRuleFile";
	item.tooltip = node.uri.fsPath;
	return item;
}

function ruleTreeItem(node: RuleNode): vscode.TreeItem {
	const rule = node.rule;
	const item = new vscode.TreeItem(rule.id, vscode.TreeItemCollapsibleState.None);
	item.description = rule.scope;
	item.iconPath = new vscode.ThemeIcon(
		rule.severity === "warn" ? "warning" : "shield",
	);
	item.contextValue = "cmRule";
	item.tooltip = rule.blockText;
	item.command = {
		command: "codeMoniker.revealRule",
		title: "Reveal",
		arguments: [node],
	};
	return item;
}
