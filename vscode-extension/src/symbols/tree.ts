import * as path from "node:path";
import * as vscode from "vscode";

import { SymbolDto, TreeNode } from "../daemon/model";
import { DaemonSession } from "../daemon/session";
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

	async getChildren(node?: SymbolTreeNode): Promise<SymbolTreeNode[]> {
		if (!node) {
			if (!this.session.ready) {
				return [info(this.notReadyLabel())];
			}
			return wrapEntries(await this.repository.topLevelEntries());
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
		return this.repository.fileSymbols(node.tree.path);
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
		item.iconPath = isDir
			? new vscode.ThemeIcon("folder")
			: fileViolations > 0
				? warningIcon()
				: new vscode.ThemeIcon("file-code");
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
		item.iconPath = violations > 0 ? warningIcon() : symbolIcon(symbol.kind);
		item.contextValue = "cmSymbol";
		item.tooltip = symbolTooltip(symbol);
		// No `command`: selection drives the detail webview; opening the file is an
		// explicit action so navigation never forces an editor open.
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
	if (violations > 0) {
		parts.push(`⚠ ${violations}`);
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
		parts.push(`⚠ ${violations}`);
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

function symbolIcon(kind: string): vscode.ThemeIcon {
	const map: Record<string, string> = {
		function: "symbol-function",
		fn: "symbol-function",
		method: "symbol-method",
		struct: "symbol-structure",
		class: "symbol-class",
		interface: "symbol-interface",
		trait: "symbol-interface",
		enum: "symbol-enum",
		field: "symbol-field",
		property: "symbol-property",
		constant: "symbol-constant",
		const: "symbol-constant",
		variable: "symbol-variable",
		module: "symbol-namespace",
		mod: "symbol-namespace",
		namespace: "symbol-namespace",
		type: "symbol-type-parameter",
		impl: "symbol-misc",
	};
	return new vscode.ThemeIcon(map[kind] ?? "symbol-misc");
}

function warningIcon(): vscode.ThemeIcon {
	return new vscode.ThemeIcon("warning", new vscode.ThemeColor("list.warningForeground"));
}
