import * as vscode from "vscode";

import { IdentitySegmentDto, SymbolDto } from "../daemon/model";
import { DaemonSession } from "../daemon/session";
import { sourceFolderIcon, statusIcon, symbolIcon } from "../shared/appIcons";
import { IdentityNode, InfoNode, SymbolNode, SymbolTreeNode } from "./nodes";
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
			const entries = await this.repository.identityChildren("");
			if (this.shapeFilter.length > 0) {
				return [info(`filter: shape = ${this.shapeFilter.join(", ")}`), ...entries];
			}
			return entries;
		}
		if (node.kind === "info") {
			return [];
		}
		if (node.kind === "identity") {
			return this.repository.identityChildren(node.row.identity);
		}
		if (node.identity && node.hasChildren) {
			return this.repository.identityChildren(node.identity);
		}
		return markLoneChild(node.children);
	}

	getTreeItem(node: SymbolTreeNode): vscode.TreeItem {
		if (node.kind === "info") {
			return new vscode.TreeItem(node.label);
		}
		if (node.kind === "identity") {
			return identityItem(node);
		}
		return this.symbolItem(node);
	}

	private symbolItem(node: SymbolNode): vscode.TreeItem {
		const symbol = node.symbol;
		const hasChildren = node.children.length > 0 || Boolean(node.hasChildren);
		const collapsible = hasChildren
			? node.expand
				? vscode.TreeItemCollapsibleState.Expanded
				: vscode.TreeItemCollapsibleState.Collapsed
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

// A lone child unrolls automatically, so single-symbol files and single-member
// containers open down to the leaf without repeated clicks.
function markLoneChild(nodes: SymbolTreeNode[]): SymbolTreeNode[] {
	if (nodes.length === 1 && nodes[0].kind === "symbol") {
		nodes[0].expand = true;
	}
	return nodes;
}

function identityItem(node: IdentityNode): vscode.TreeItem {
	const collapsible = node.expand
		? vscode.TreeItemCollapsibleState.Expanded
		: vscode.TreeItemCollapsibleState.Collapsed;
	const item = new vscode.TreeItem(node.label ?? node.row.name, collapsible);
	item.description = identityDescription(node.row);
	item.iconPath = sourceFolderIcon();
	item.contextValue = "cmIdentity";
	item.tooltip = node.row.identity;
	return item;
}

function identityDescription(row: IdentitySegmentDto): string {
	const parts = [row.kind];
	if (row.defs > 0) {
		parts.push(`${row.defs} defs`);
	}
	return parts.join(" · ");
}

function info(label: string): InfoNode {
	return { kind: "info", label };
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
