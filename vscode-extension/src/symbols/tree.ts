import * as path from "node:path";
import * as vscode from "vscode";

import { SymbolDto, TreeNode } from "../daemon/model";
import { DaemonSession } from "../daemon/session";
import { sourceFileIcon, sourceFolderIcon, statusIcon, symbolIcon } from "../shared/appIcons";
import { EntryNode, InfoNode, SymbolNode, SymbolTreeNode } from "./nodes";
import { SymbolRepository } from "./repository";

// Optional violation overlay supplied by the rules feature so symbol/file rows can
// surface check findings without coupling the two trees.
export interface ViolationIndex {
	fileViolations(filePath: string): number;
	symbolViolations(symbol: SymbolDto): number;
}

export class SymbolTreeProvider implements vscode.TreeDataProvider<SymbolTreeNode> {
	private readonly emitter = new vscode.EventEmitter<SymbolTreeNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;
	private violations?: ViolationIndex;
	private shapeFilter: string[] = [];

	constructor(
		private readonly session: DaemonSession,
		private readonly repository: SymbolRepository,
	) {}

	refresh(): void {
		this.emitter.fire(undefined);
	}

	setViolations(index: ViolationIndex | undefined): void {
		this.violations = index;
		this.emitter.fire(undefined);
	}

	get shapes(): string[] {
		return this.shapeFilter;
	}

	setShapeFilter(shapes: string[]): void {
		this.shapeFilter = shapes;
		this.emitter.fire(undefined);
	}

	async getChildren(node?: SymbolTreeNode): Promise<SymbolTreeNode[]> {
		if (!node) {
			if (!this.session.ready) {
				return [info(this.notReadyLabel())];
			}
			const entries = wrapEntries(await this.repository.topLevelEntries());
			if (this.shapeFilter.length > 0) {
				return [info(`filter: shape = ${this.shapeFilter.join(", ")}`), ...entries];
			}
			return entries;
		}
		if (node.kind === "info") {
			return [];
		}
		if (node.kind === "symbol") {
			return node.children;
		}
		if (node.tree.kind === "directory") {
			return wrapEntries(await this.repository.childEntries(node.tree.path));
		}
		return this.repository.fileSymbols(node.tree.path, this.shapeFilter);
	}

	getTreeItem(node: SymbolTreeNode): vscode.TreeItem {
		if (node.kind === "info") {
			return new vscode.TreeItem(node.label);
		}
		if (node.kind === "entry") {
			return this.entryItem(node);
		}
		return this.symbolItem(node);
	}

	private entryItem(node: EntryNode): vscode.TreeItem {
		const label = path.basename(node.tree.path) || node.tree.path;
		const isDir = node.tree.kind === "directory";
		const collapsible = isDir || node.tree.defs > 0
			? vscode.TreeItemCollapsibleState.Collapsed
			: vscode.TreeItemCollapsibleState.None;
		const item = new vscode.TreeItem(label, collapsible);
		const fileViolations = isDir ? 0 : this.violations?.fileViolations(node.tree.path) ?? 0;
		item.description = entryDescription(node, fileViolations);
		if (isDir) {
			item.iconPath = sourceFolderIcon();
		} else if (fileViolations > 0) {
			item.iconPath = statusIcon("warning");
		} else {
			item.iconPath = sourceFileIcon();
		}
		item.contextValue = isDir ? "cmEntryDir" : "cmEntryFile";
		item.tooltip = node.tree.path;
		if (!isDir) {
			item.resourceUri = vscode.Uri.file(path.join(node.tree.root, node.tree.path));
		}
		return item;
	}

	private symbolItem(node: SymbolNode): vscode.TreeItem {
		const symbol = node.symbol;
		const collapsible = node.children.length > 0
			? vscode.TreeItemCollapsibleState.Collapsed
			: vscode.TreeItemCollapsibleState.None;
		const item = new vscode.TreeItem(symbol.name, collapsible);
		const violations = this.violations?.symbolViolations(symbol) ?? 0;
		item.description = symbolDescription(symbol, violations);
		item.iconPath = violations > 0 ? statusIcon("warning") : symbolIcon(symbol.kind);
		item.contextValue = "cmSymbol";
		item.tooltip = symbolTooltip(symbol);
		return item;
	}

	private notReadyLabel(): string {
		switch (this.session.status) {
			case "loading":
				return "Indexing workspace…";
			case "connecting":
				return "Connecting to daemon…";
			case "error":
				return `Daemon error: ${this.session.lastError ?? "unknown"}`;
			default:
				return "Daemon not connected";
		}
	}
}

function wrapEntries(entries: TreeNode[]): EntryNode[] {
	return entries.map((tree) => ({ kind: "entry", tree }));
}

function info(label: string): InfoNode {
	return { kind: "info", label };
}

function entryDescription(node: EntryNode, violations: number): string {
	const parts: string[] = [];
	if (node.tree.defs > 0) {
		parts.push(`${node.tree.defs} defs`);
	}
	if (node.tree.refs > 0) {
		parts.push(`${node.tree.refs} refs`);
	}
	if (node.tree.change_count > 0) {
		parts.push(`±${node.tree.change_count}`);
	}
	if (violations > 0) {
		parts.push(`${violations} violation(s)`);
	}
	return parts.join(" · ");
}

function symbolDescription(symbol: SymbolDto, violations: number): string {
	const parts = [symbol.kind];
	if (symbol.visibility && symbol.visibility !== "default") {
		parts.push(symbol.visibility);
	}
	if (symbol.line_range) {
		parts.push(`L${symbol.line_range[0]}`);
	}
	if (violations > 0) {
		parts.push(`${violations} violation(s)`);
	}
	return parts.join(" · ");
}

function symbolTooltip(symbol: SymbolDto): vscode.MarkdownString {
	const md = new vscode.MarkdownString();
	md.appendMarkdown(`**${symbol.kind} ${symbol.name}**\n\n`);
	if (symbol.signature) {
		md.appendCodeblock(symbol.signature, symbol.language);
	}
	md.appendMarkdown(`\n- file: \`${symbol.file}\`\n`);
	md.appendMarkdown(`- moniker: \`${symbol.uri}\`\n`);
	return md;
}
