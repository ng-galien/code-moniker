import * as vscode from "vscode";

import { CheckReport } from "../cli/model";
import { workspaceLabel } from "../shared/workspace";
import { RuleFileNode, RuleFolderNode, RuleNode, RuleTreeNode } from "./nodes";
import { RuleEntry } from "./parse";
import { findRuleFiles } from "./repository";

type FileFeedback = {
	validation?: "clean" | "failed";
	run?: {
		status: "clean" | "violations" | "failed";
		violations?: number;
	};
	ruleHits: Map<string, number>;
};

export class RuleFilesProvider implements vscode.TreeDataProvider<RuleTreeNode> {
	private readonly emitter = new vscode.EventEmitter<RuleTreeNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;
	private readonly feedback = new Map<string, FileFeedback>();

	refresh(): void {
		this.emitter.fire(undefined);
	}

	markValidation(uri: vscode.Uri, status: "clean" | "failed"): void {
		this.feedbackFor(uri).validation = status;
		this.emitter.fire(undefined);
	}

	markRunFailed(uri: vscode.Uri): void {
		const feedback = this.feedbackFor(uri);
		feedback.run = { status: "failed" };
		feedback.ruleHits = new Map();
		this.emitter.fire(undefined);
	}

	markRun(uri: vscode.Uri, report: CheckReport): void {
		this.markRunForFiles([{ kind: "file", uri, parsed: { rules: [], aliases: [], profiles: [] } }], report);
	}

	markRunForFiles(files: RuleFileNode[], report: CheckReport): void {
		const hits = ruleHits(report);
		for (const file of files) {
			const feedback = this.feedbackFor(file.uri);
			feedback.ruleHits = hits;
			const violations = fileRuleViolations(file, hits);
			feedback.run = violations > 0
				? { status: "violations", violations }
				: { status: "clean", violations: 0 };
		}
		this.emitter.fire(undefined);
	}

	async getChildren(node?: RuleTreeNode): Promise<RuleTreeNode[]> {
		if (!node) {
			const files = await findRuleFiles();
			if (files.length === 0) {
				return [{ kind: "info", label: "No .code-moniker.toml or *.fragment.toml found" }];
			}
			return ruleFileTree(files);
		}
		if (node.kind === "folder") {
			return node.children;
		}
		if (node.kind === "file") {
			if (node.parsed.rules.length === 0) {
				return [{ kind: "info", label: "No rules in this file" }];
			}
			return node.parsed.rules.map((rule) => ({
				kind: "rule",
				uri: node.uri,
				fileFragment: node.parsed.fragment,
				rule,
			}));
		}
		return [];
	}

	getTreeItem(node: RuleTreeNode): vscode.TreeItem {
		if (node.kind === "info") {
			return new vscode.TreeItem(node.label);
		}
		if (node.kind === "file") {
			return ruleFileTreeItem(node, this.feedback.get(node.uri.toString()));
		}
		if (node.kind === "folder") {
			return ruleFolderTreeItem(node, this.feedback);
		}
		return ruleTreeItem(node, this.feedback.get(node.uri.toString()));
	}

	private feedbackFor(uri: vscode.Uri): FileFeedback {
		const key = uri.toString();
		let feedback = this.feedback.get(key);
		if (!feedback) {
			feedback = { ruleHits: new Map() };
			this.feedback.set(key, feedback);
		}
		return feedback;
	}
}

function ruleHits(report: CheckReport): Map<string, number> {
	const hits = new Map<string, number>();
	for (const file of report.files) {
		for (const violation of file.violations) {
			hits.set(violation.rule_id, (hits.get(violation.rule_id) ?? 0) + 1);
		}
	}
	return hits;
}

function fileRuleViolations(file: RuleFileNode, hits: Map<string, number>): number {
	return file.parsed.rules.reduce(
		(total, rule) => total + (hits.get(effectiveRuleId(rule, file.parsed.fragment)) ?? 0),
		0,
	);
}

function ruleFileTree(files: RuleFileNode[]): RuleTreeNode[] {
	const roots = new Map<string, RuleFolderNode>();
	const looseFiles: RuleTreeNode[] = [];
	for (const file of files) {
		const parts = workspaceLabel(file.uri).split(/[\\/]+/).filter(Boolean);
		if (parts.length <= 1) {
			looseFiles.push(file);
			continue;
		}
		let children: RuleTreeNode[] = rootChildren(roots, parts[0]);
		let relativePath = parts[0];
		for (const folder of parts.slice(1, -1)) {
			relativePath = `${relativePath}/${folder}`;
			children = folderChildren(children, folder, relativePath);
		}
		children.push(file);
	}
	return sortRuleTree([...looseFiles, ...roots.values()]);
}

function rootChildren(roots: Map<string, RuleFolderNode>, label: string): RuleTreeNode[] {
	let node = roots.get(label);
	if (!node) {
		node = folderNode(label, label);
		roots.set(label, node);
	}
	return node.children;
}

function folderChildren(
	siblings: RuleTreeNode[],
	label: string,
	relativePath: string,
): RuleTreeNode[] {
	let node = siblings.find(
		(child): child is RuleFolderNode =>
			child.kind === "folder" && child.relativePath === relativePath,
	);
	if (!node) {
		node = folderNode(label, relativePath);
		siblings.push(node);
	}
	return node.children;
}

function folderNode(label: string, relativePath: string): RuleFolderNode {
	return {
		kind: "folder",
		id: `folder:${relativePath}`,
		label,
		relativePath,
		children: [],
	};
}

function sortRuleTree(nodes: RuleTreeNode[]): RuleTreeNode[] {
	const sorted = [...nodes].sort((left, right) => {
		if (left.kind === "folder" && right.kind !== "folder") {
			return -1;
		}
		if (left.kind !== "folder" && right.kind === "folder") {
			return 1;
		}
		return nodeLabel(left).localeCompare(nodeLabel(right));
	});
	for (const node of sorted) {
		if (node.kind === "folder") {
			node.children = sortRuleTree(node.children);
		}
	}
	return sorted;
}

function nodeLabel(node: RuleTreeNode): string {
	if (node.kind === "folder" || node.kind === "info") {
		return node.label;
	}
	if (node.kind === "file") {
		return workspaceLabel(node.uri);
	}
	return node.rule.id;
}

function ruleFolderTreeItem(
	node: RuleFolderNode,
	feedback: Map<string, FileFeedback>,
): vscode.TreeItem {
	const item = new vscode.TreeItem(
		node.label,
		vscode.TreeItemCollapsibleState.Collapsed,
	);
	item.id = node.id;
	item.description = folderDescription(node, feedback);
	item.iconPath = folderIcon(node, feedback);
	item.contextValue = "cmRuleFolder";
	item.tooltip = folderTooltip(node, feedback);
	return item;
}

function ruleFileTreeItem(node: RuleFileNode, feedback: FileFeedback | undefined): vscode.TreeItem {
	const item = new vscode.TreeItem(
		fileLabel(node.uri),
		vscode.TreeItemCollapsibleState.Collapsed,
	);
	item.description = fileDescription(node, feedback);
	item.resourceUri = node.uri;
	item.iconPath = fileIcon(feedback);
	item.contextValue = "cmRuleFile";
	item.tooltip = fileTooltip(node, feedback);
	return item;
}

function fileLabel(uri: vscode.Uri): string {
	const label = workspaceLabel(uri).split(/[\\/]+/).filter(Boolean).pop();
	return label ?? workspaceLabel(uri);
}

function folderDescription(node: RuleFolderNode, feedback: Map<string, FileFeedback>): string {
	const files = countFiles(node);
	const violations = folderViolations(node, feedback);
	if (violations > 0) {
		return `${files} file(s) · ${violations} violation(s)`;
	}
	if (folderHasRunFailure(node, feedback)) {
		return `${files} file(s) · run failed`;
	}
	if (folderHasCleanRun(node, feedback)) {
		return `${files} file(s) · clean`;
	}
	return `${files} file(s)`;
}

function countFiles(node: RuleFolderNode): number {
	return node.children.reduce((count, child) => {
		if (child.kind === "file") {
			return count + 1;
		}
		if (child.kind === "folder") {
			return count + countFiles(child);
		}
		return count;
	}, 0);
}

function folderIcon(node: RuleFolderNode, feedback: Map<string, FileFeedback>): vscode.ThemeIcon {
	if (folderHasRunFailure(node, feedback)) {
		return statusIcon("error");
	}
	if (folderViolations(node, feedback) > 0) {
		return statusIcon("warning");
	}
	if (folderHasCleanRun(node, feedback)) {
		return statusIcon("pass");
	}
	return new vscode.ThemeIcon("folder");
}

function folderTooltip(node: RuleFolderNode, feedback: Map<string, FileFeedback>): string {
	const lines = [node.relativePath];
	const violations = folderViolations(node, feedback);
	if (violations > 0) {
		lines.push(`last run: ${violations} violation(s)`);
	} else if (folderHasRunFailure(node, feedback)) {
		lines.push("last run: failed");
	} else if (folderHasCleanRun(node, feedback)) {
		lines.push("last run: clean");
	}
	return lines.join("\n");
}

function folderViolations(node: RuleFolderNode, feedback: Map<string, FileFeedback>): number {
	return node.children.reduce((total, child) => {
		if (child.kind === "file") {
			return total + (feedback.get(child.uri.toString())?.run?.violations ?? 0);
		}
		if (child.kind === "folder") {
			return total + folderViolations(child, feedback);
		}
		return total;
	}, 0);
}

function folderHasRunFailure(node: RuleFolderNode, feedback: Map<string, FileFeedback>): boolean {
	return node.children.some((child) => {
		if (child.kind === "file") {
			return feedback.get(child.uri.toString())?.run?.status === "failed";
		}
		return child.kind === "folder" && folderHasRunFailure(child, feedback);
	});
}

function folderHasCleanRun(node: RuleFolderNode, feedback: Map<string, FileFeedback>): boolean {
	return node.children.some((child) => {
		if (child.kind === "file") {
			return feedback.get(child.uri.toString())?.run?.status === "clean";
		}
		return child.kind === "folder" && folderHasCleanRun(child, feedback);
	});
}

function ruleTreeItem(node: RuleNode, feedback: FileFeedback | undefined): vscode.TreeItem {
	const rule = node.rule;
	const item = new vscode.TreeItem(rule.id, vscode.TreeItemCollapsibleState.None);
	const hits = hitsForRule(node, feedback);
	item.description = hits === undefined ? rule.scope : `${rule.scope} · ${hits} hit(s)`;
	item.iconPath = ruleIcon(rule, hits);
	item.contextValue = "cmRule";
	item.tooltip = ruleTooltip(node, hits);
	item.command = {
		command: "codeMoniker.revealRule",
		title: "Reveal",
		arguments: [node],
	};
	return item;
}

function fileDescription(node: RuleFileNode, feedback: FileFeedback | undefined): string {
	const parts = [`${node.parsed.rules.length} rule(s)`];
	if (feedback?.validation === "clean") {
		parts.push("valid");
	} else if (feedback?.validation === "failed") {
		parts.push("invalid");
	}
	if (feedback?.run?.status === "clean") {
		parts.push("clean");
	} else if (feedback?.run?.status === "violations") {
		parts.push(`${feedback.run.violations ?? 0} violation(s)`);
	} else if (feedback?.run?.status === "failed") {
		parts.push("run failed");
	}
	return parts.join(" · ");
}

function fileIcon(feedback: FileFeedback | undefined): vscode.ThemeIcon {
	if (feedback?.validation === "failed" || feedback?.run?.status === "failed") {
		return statusIcon("error");
	}
	if (feedback?.run?.status === "violations") {
		return statusIcon("warning");
	}
	if (feedback?.validation === "clean" || feedback?.run?.status === "clean") {
		return statusIcon("pass");
	}
	return new vscode.ThemeIcon("law");
}

function fileTooltip(node: RuleFileNode, feedback: FileFeedback | undefined): string {
	const lines = [node.uri.fsPath];
	if (node.parsed.fragment) {
		lines.push(`fragment: ${node.parsed.fragment}`);
	}
	if (feedback?.validation) {
		lines.push(`validation: ${feedback.validation}`);
	}
	if (feedback?.run) {
		lines.push(`last run: ${feedback.run.status}`);
	}
	return lines.join("\n");
}

function hitsForRule(node: RuleNode, feedback: FileFeedback | undefined): number | undefined {
	if (!feedback?.run) {
		return undefined;
	}
	return feedback.ruleHits.get(effectiveRuleId(node.rule, node.fileFragment)) ?? 0;
}

function effectiveRuleId(rule: RuleEntry, fragment: string | undefined): string {
	return fragment ? `${rule.scope}.${fragment}.${rule.id}` : `${rule.scope}.${rule.id}`;
}

function ruleIcon(rule: RuleEntry, hits: number | undefined): vscode.ThemeIcon {
	if (hits !== undefined && hits > 0) {
		return statusIcon("warning");
	}
	if (hits === 0) {
		return statusIcon("pass");
	}
	return rule.severity === "warn"
		? statusIcon("warning")
		: new vscode.ThemeIcon("shield", new vscode.ThemeColor("charts.blue"));
}

function ruleTooltip(node: RuleNode, hits: number | undefined): string {
	const lines = [];
	if (hits !== undefined) {
		lines.push(`last run: ${hits} hit(s)`);
		lines.push("");
	}
	lines.push(node.rule.blockText);
	return lines.join("\n");
}

function statusIcon(status: "error" | "pass" | "warning"): vscode.ThemeIcon {
	if (status === "error") {
		return new vscode.ThemeIcon("error", new vscode.ThemeColor("errorForeground"));
	}
	if (status === "warning") {
		return new vscode.ThemeIcon("warning", new vscode.ThemeColor("list.warningForeground"));
	}
	return new vscode.ThemeIcon("pass", new vscode.ThemeColor("testing.iconPassed"));
}
