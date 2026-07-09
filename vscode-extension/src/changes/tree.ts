import * as vscode from "vscode";

import { ChangeReviewFile, ChangeReviewResult, ChangeReviewSymbol } from "../daemon/model";
import { DaemonSession } from "../daemon/session";
import { changeSymbolIcon, statusIcon } from "../shared/appIcons";
import { ChangeFileNode, ChangeTreeNode, changeFilePath, changeSymbolName } from "./nodes";
import { ChangesRepository } from "./repository";

// The "Changes" section of the workspace tree: semantic change facts for
// HEAD..worktree. Facts only — dispositions, per-symbol kinds, confidence and
// residual coverage are shown verbatim, never filtered by importance.
export class ChangesProvider implements vscode.TreeDataProvider<ChangeTreeNode> {
	private readonly emitter = new vscode.EventEmitter<ChangeTreeNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;

	constructor(
		private readonly session: DaemonSession,
		private readonly repository: ChangesRepository,
	) {}

	refresh(): void {
		this.emitter.fire(undefined);
	}

	async getChildren(node?: ChangeTreeNode): Promise<ChangeTreeNode[]> {
		if (!node) {
			return this.rootChildren();
		}
		if (node.kind !== "file") {
			return [];
		}
		return fileChildren(node);
	}

	getTreeItem(node: ChangeTreeNode): vscode.TreeItem {
		switch (node.kind) {
			case "file":
				return fileItem(node.file);
			case "symbolChange":
				return symbolItem(node.change);
			case "info":
				return new vscode.TreeItem(node.label);
		}
	}

	private async rootChildren(): Promise<ChangeTreeNode[]> {
		if (!this.session.ready) {
			return [{ kind: "info", label: "Daemon not connected" }];
		}
		const review = await this.repository.review();
		if (!review) {
			return [{ kind: "info", label: "Change review unavailable" }];
		}
		if (review.files.length === 0) {
			return [{ kind: "info", label: "No changes against HEAD" }];
		}
		return review.files.map((file) => ({ kind: "file", file, review }));
	}
}

function fileChildren(node: ChangeFileNode): ChangeTreeNode[] {
	const paths = new Set(
		[node.file.old_path, node.file.new_path].filter((path): path is string => path != null),
	);
	const changes = node.review.symbol_changes.filter((change) => {
		const path = change.new?.file ?? change.old?.file;
		return path != null && paths.has(path);
	});
	const children: ChangeTreeNode[] = changes.map((change) => ({ kind: "symbolChange", change }));
	const refs = node.review.ref_changes.filter((change) => paths.has(change.file)).length;
	if (refs > 0) {
		children.push({ kind: "info", label: `${refs} reference fact(s)` });
	}
	if (!node.file.coverage_explained) {
		children.push({ kind: "info", label: "residual: unattributed edits" });
	}
	return children;
}

function fileItem(file: ChangeReviewFile): vscode.TreeItem {
	const label = fileLabel(file);
	const collapsible = file.analyzable
		? vscode.TreeItemCollapsibleState.Collapsed
		: vscode.TreeItemCollapsibleState.None;
	const item = new vscode.TreeItem(label, collapsible);
	item.description = fileDescription(file);
	item.contextValue = "cmChangeFile";
	item.iconPath = file.coverage_explained
		? changeSymbolIcon(file.disposition)
		: statusIcon("warning");
	item.tooltip = fileTooltip(file);
	return item;
}

function fileLabel(file: ChangeReviewFile): string {
	if (file.old_path && file.new_path && file.old_path !== file.new_path) {
		return `${file.old_path} → ${file.new_path}`;
	}
	return changeFilePath(file);
}

function fileDescription(file: ChangeReviewFile): string {
	const parts = [file.disposition];
	if (!file.analyzable) {
		parts.push("not analyzable");
	}
	if (file.symbol_changes > 0) {
		parts.push(`${file.symbol_changes} symbol(s)`);
	}
	if (file.moved_symbols > 0) {
		parts.push(`${file.moved_symbols} moved`);
	}
	if (!file.coverage_explained) {
		parts.push("residual");
	}
	return parts.join(" · ");
}

function fileTooltip(file: ChangeReviewFile): string {
	const lines = [fileLabel(file), `disposition: ${file.disposition}`];
	if (!file.coverage_explained) {
		lines.push("some changed lines are not attributed to any symbolic fact");
	}
	return lines.join("\n");
}

function symbolItem(change: ChangeReviewSymbol): vscode.TreeItem {
	const item = new vscode.TreeItem(changeSymbolName(change), vscode.TreeItemCollapsibleState.None);
	item.description = symbolDescription(change);
	item.contextValue = "cmChangeSymbol";
	item.iconPath = changeSymbolIcon(change.kind);
	return item;
}

function symbolDescription(change: ChangeReviewSymbol): string {
	const parts = [change.kind];
	const facets = facetLabels(change);
	if (facets.length > 0) {
		parts.push(`+${facets.join(" +")}`);
	}
	if (change.confidence !== "certain") {
		parts.push(`[${change.confidence}]`);
	}
	return parts.join(" ");
}

function facetLabels(change: ChangeReviewSymbol): string[] {
	const facets: string[] = [];
	if (change.body_changed && change.kind !== "body-modified") {
		facets.push("body");
	}
	if (change.signature_changed && change.kind !== "signature-changed") {
		facets.push("signature");
	}
	if (change.visibility_changed) {
		facets.push("visibility");
	}
	if (change.header_changed) {
		facets.push("header");
	}
	return facets;
}

export function reviewSummaryLabel(review: ChangeReviewResult): string {
	const summary = review.summary;
	return (
		`${summary.files} file(s) · ${summary.symbol_changes} symbol fact(s) · ` +
		`${summary.retargeted_refs} retargeted ref(s) · ${summary.residual_files} residual`
	);
}
