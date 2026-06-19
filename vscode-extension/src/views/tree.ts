import * as vscode from "vscode";

import { ViewBoundaryDto, ViewGotchaDto } from "../daemon/model";
import {
	ViewDetail,
	ViewBoundaryNode,
	ViewEvidenceNode,
	ViewGotchaNode,
	ViewRuleNode,
	ViewSectionNode,
	ViewSummaryNode,
	ViewTreeNode,
} from "./nodes";
import { ViewsRepository } from "./repository";

export class ViewsProvider implements vscode.TreeDataProvider<ViewTreeNode> {
	private readonly emitter = new vscode.EventEmitter<ViewTreeNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;
	private readonly detailCache = new Map<string, ViewDetail>();

	constructor(private readonly repository: ViewsRepository) {}

	refresh(): void {
		this.detailCache.clear();
		this.emitter.fire(undefined);
	}

	async getChildren(node?: ViewTreeNode): Promise<ViewTreeNode[]> {
		if (!node) {
			if (!this.repository.ready) {
				return [{ kind: "info", label: "Daemon not ready" }];
			}
			const views = await this.repository.listViews();
			return views.length > 0
				? views.map((view) => ({ kind: "view", view }))
				: [{ kind: "info", label: "No code-moniker views found" }];
		}
		if (node.kind === "view") {
			const detail = await this.detail(node.view.id);
			return detail ? sections(detail) : [{ kind: "info", label: "View not found" }];
		}
		if (node.kind === "section") {
			return sectionChildren(node);
		}
		if (node.kind === "boundary") {
			return evidenceChildren(node.boundary);
		}
		if (node.kind === "gotcha") {
			return evidenceChildren(node.gotcha);
		}
		return [];
	}

	getTreeItem(node: ViewTreeNode): vscode.TreeItem {
		switch (node.kind) {
			case "view":
				return viewItem(node);
			case "section":
				return sectionItem(node);
			case "boundary":
				return boundaryItem(node);
			case "gotcha":
				return gotchaItem(node);
			case "evidence":
				return evidenceItem(node);
			case "rule":
				return ruleItem(node);
			default:
				return new vscode.TreeItem(node.label);
		}
	}

	private async detail(id: string): Promise<ViewDetail | undefined> {
		let detail = this.detailCache.get(id);
		if (!detail) {
			detail = await this.repository.readView(id);
			if (detail) {
				this.detailCache.set(id, detail);
			}
		}
		return detail;
	}
}

function sections(view: ViewDetail): ViewSectionNode[] {
	return [
		{ kind: "section", id: "rules", label: "Rules", view },
		{ kind: "section", id: "boundaries", label: "Boundaries", view },
		{ kind: "section", id: "gotchas", label: "Gotchas", view },
	];
}

function sectionChildren(node: ViewSectionNode): ViewTreeNode[] {
	switch (node.id) {
		case "rules":
			return node.view.rules.length > 0
				? node.view.rules.map((rule) => ({ kind: "rule", rule }))
				: [{ kind: "info", label: "No rules referenced" }];
		case "boundaries":
			return node.view.boundaries.length > 0
				? node.view.boundaries.map((boundary) => ({ kind: "boundary", boundary }))
				: [{ kind: "info", label: "No boundaries" }];
		case "gotchas":
			return node.view.gotchas.length > 0
				? node.view.gotchas.map((gotcha) => ({ kind: "gotcha", gotcha }))
				: [{ kind: "info", label: "No gotchas" }];
	}
}

function evidenceChildren(node: ViewBoundaryDto | ViewGotchaDto): ViewTreeNode[] {
	const children: ViewTreeNode[] = node.evidence.map((evidence) => ({ kind: "evidence", evidence }));
	for (const missing of node.missing) {
		children.push({ kind: "info", label: `Missing: ${missing}` });
	}
	return children;
}

function viewItem(node: ViewSummaryNode): vscode.TreeItem {
	const label = node.view.title ?? node.view.id;
	const item = new vscode.TreeItem(label, vscode.TreeItemCollapsibleState.Collapsed);
	item.description = [node.view.fragment, node.view.scope].filter(Boolean).join(" · ");
	item.tooltip = `${node.view.id}\n${node.view.anchor}`;
	item.iconPath = new vscode.ThemeIcon("references");
	item.contextValue = "cmView";
	return item;
}

function sectionItem(node: ViewSectionNode): vscode.TreeItem {
	const item = new vscode.TreeItem(node.label, vscode.TreeItemCollapsibleState.Collapsed);
	item.description = sectionCount(node);
	item.iconPath = new vscode.ThemeIcon(sectionIcon(node.id));
	return item;
}

function sectionCount(node: ViewSectionNode): string {
	switch (node.id) {
		case "rules":
			return String(node.view.rules.length);
		case "boundaries":
			return String(node.view.boundaries.length);
		case "gotchas":
			return String(node.view.gotchas.length);
	}
}

function sectionIcon(id: ViewSectionNode["id"]): string {
	switch (id) {
		case "rules":
			return "law";
		case "boundaries":
			return "symbol-interface";
		case "gotchas":
			return "warning";
	}
}

function boundaryItem(node: ViewBoundaryNode): vscode.TreeItem {
	const item = new vscode.TreeItem(node.boundary.id, vscode.TreeItemCollapsibleState.Collapsed);
	item.description = `${node.boundary.evidence.length} evidence`;
	item.iconPath = new vscode.ThemeIcon("symbol-interface");
	item.tooltip = node.boundary.rationale ?? undefined;
	return item;
}

function gotchaItem(node: ViewGotchaNode): vscode.TreeItem {
	const item = new vscode.TreeItem(node.gotcha.id, vscode.TreeItemCollapsibleState.Collapsed);
	item.description = node.gotcha.check ?? `${node.gotcha.evidence.length} evidence`;
	item.iconPath = new vscode.ThemeIcon("warning");
	item.tooltip = node.gotcha.rationale;
	return item;
}

function evidenceItem(node: ViewEvidenceNode): vscode.TreeItem {
	const item = new vscode.TreeItem(node.evidence.label, vscode.TreeItemCollapsibleState.None);
	item.description = node.evidence.file;
	item.iconPath = new vscode.ThemeIcon("symbol-misc");
	item.tooltip = node.evidence.moniker;
	return item;
}

function ruleItem(node: ViewRuleNode): vscode.TreeItem {
	const item = new vscode.TreeItem(node.rule.id, vscode.TreeItemCollapsibleState.None);
	item.description = [node.rule.severity, node.rule.domain].filter(Boolean).join(" · ");
	item.iconPath = new vscode.ThemeIcon("shield");
	item.tooltip = node.rule.rationale ?? undefined;
	return item;
}
