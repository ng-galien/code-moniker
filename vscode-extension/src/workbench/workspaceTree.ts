import * as vscode from "vscode";

import { ChangeTreeNode } from "../changes/nodes";
import { ChangesProvider } from "../changes/tree";
import { DaemonNode } from "../daemon/nodes";
import { DaemonListProvider } from "../daemon/tree";
import { RuleTreeNode } from "../rules/nodes";
import { RuleFilesProvider } from "../rules/tree";
import { RulesTreeNode } from "../rules-daemon/nodes";
import { RulesProvider } from "../rules-daemon/tree";
import { SetupTreeNode } from "../setup/nodes";
import { SetupProvider } from "../setup/tree";
import { workspaceSectionIcon } from "../shared/appIcons";
import { SymbolTreeNode } from "../symbols/nodes";
import { SymbolTreeProvider } from "../symbols/tree";
import { ViewTreeNode } from "../views/nodes";
import { ViewsProvider } from "../views/tree";
import { workspaceNodeId } from "./nodeIds";

type SectionId = "setup" | "daemon" | "symbols" | "views" | "changes" | "check" | "ruleFiles";

interface SectionNode {
	kind: "section";
	id: SectionId;
	label: string;
}

export interface WorkspaceSetupNode {
	kind: "setup";
	node: SetupTreeNode;
}

export interface WorkspaceDaemonNode {
	kind: "daemon";
	node: DaemonNode;
}

export interface WorkspaceSymbolNode {
	kind: "symbols";
	node: SymbolTreeNode;
}

export interface WorkspaceRulesNode {
	kind: "check";
	node: RulesTreeNode;
}

export interface WorkspaceChangeNode {
	kind: "changes";
	node: ChangeTreeNode;
}

export interface WorkspaceViewNode {
	kind: "views";
	node: ViewTreeNode;
}

export interface WorkspaceRuleFileNode {
	kind: "ruleFiles";
	node: RuleTreeNode;
}

export type WorkspaceNode =
	| SectionNode
	| WorkspaceSetupNode
	| WorkspaceDaemonNode
	| WorkspaceSymbolNode
	| WorkspaceViewNode
	| WorkspaceChangeNode
	| WorkspaceRulesNode
	| WorkspaceRuleFileNode;

const REFRESH_COALESCE_MS = 50;

export class WorkspaceTreeProvider implements vscode.TreeDataProvider<WorkspaceNode>, vscode.Disposable {
	private readonly emitter = new vscode.EventEmitter<WorkspaceNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;
	private readonly subscriptions: vscode.Disposable[] = [];
	private readonly parents = new WeakMap<object, WorkspaceNode | undefined>();
	private pendingRefresh?: NodeJS.Timeout;

	constructor(
		private readonly setup: SetupProvider,
		private readonly daemons: DaemonListProvider,
		private readonly symbols: SymbolTreeProvider,
		private readonly views: ViewsProvider,
		private readonly changes: ChangesProvider,
		private readonly rules: RulesProvider,
		private readonly ruleFiles: RuleFilesProvider,
	) {
		this.subscriptions.push(
			this.setup.onDidChangeTreeData(() => this.refresh()),
			this.daemons.onDidChangeTreeData(() => this.refresh()),
			this.symbols.onDidChangeTreeData(() => this.refresh()),
			this.views.onDidChangeTreeData(() => this.refresh()),
			this.changes.onDidChangeTreeData(() => this.refresh()),
			this.rules.onDidChangeTreeData(() => this.refresh()),
			this.ruleFiles.onDidChangeTreeData(() => this.refresh()),
		);
	}

	// Child providers fire in bursts (a daemon event refreshes four of them at
	// once); coalescing into one tree invalidation per window keeps VS Code
	// from re-querying every expanded node once per provider.
	refresh(): void {
		if (this.pendingRefresh) {
			return;
		}
		this.pendingRefresh = setTimeout(() => {
			this.pendingRefresh = undefined;
			this.emitter.fire(undefined);
		}, REFRESH_COALESCE_MS);
	}

	async getChildren(node?: WorkspaceNode): Promise<WorkspaceNode[]> {
		if (!node) {
			return this.track(sections(), undefined);
		}
		if (node.kind === "section") {
			return this.track(await this.sectionChildren(node.id), node);
		}
		let children: WorkspaceNode[];
		switch (node.kind) {
			case "setup":
				children = wrapSetup(await this.setup.getChildren(node.node));
				break;
			case "daemon":
				children = wrapDaemons(await this.daemons.getChildren(node.node));
				break;
			case "symbols":
				children = wrapSymbols(await this.symbols.getChildren(node.node));
				break;
			case "views":
				children = wrapViews(await this.views.getChildren(node.node));
				break;
			case "changes":
				children = wrapChanges(await this.changes.getChildren(node.node));
				break;
			case "check":
				children = wrapRules(await this.rules.getChildren(node.node));
				break;
			case "ruleFiles":
				children = wrapRuleFiles(await this.ruleFiles.getChildren(node.node));
				break;
		}
		return this.track(children, node);
	}

	getParent(node: WorkspaceNode): vscode.ProviderResult<WorkspaceNode> {
		return this.parents.get(node);
	}

	async findSymbolPath(uri: string): Promise<WorkspaceNode[] | undefined> {
		const target = identityFromUri(uri);
		const section: SectionNode = { kind: "section", id: "symbols", label: "Symbols" };
		this.parents.set(section, undefined);
		const path: WorkspaceNode[] = [section];
		let parent: WorkspaceNode = section;
		let children = await this.symbols.getChildren();
		for (let hop = 0; hop < 64; hop++) {
			const exact = children.find(
				(node) => node.kind === "symbol" && node.symbol.uri === uri,
			);
			if (exact) {
				const wrapped: WorkspaceSymbolNode = { kind: "symbols", node: exact };
				this.parents.set(wrapped, parent);
				return [...path, wrapped];
			}
			const next = children
				.map((node) => ({ node, identity: symbolNodeIdentity(node) }))
				.filter(
					(candidate): candidate is { node: SymbolTreeNode; identity: string } =>
						Boolean(candidate.identity) &&
						(target === candidate.identity || target.startsWith(`${candidate.identity}/`)),
				)
				.sort((a, b) => b.identity.length - a.identity.length)[0];
			if (!next) return undefined;
			const wrapped: WorkspaceSymbolNode = { kind: "symbols", node: next.node };
			this.parents.set(wrapped, parent);
			path.push(wrapped);
			parent = wrapped;
			children = await this.symbols.getChildren(next.node);
		}
		return undefined;
	}

	getTreeItem(node: WorkspaceNode): vscode.TreeItem {
		const item = this.rawTreeItem(node);
		item.id ??= workspaceNodeId(node);
		return item;
	}

	dispose(): void {
		if (this.pendingRefresh) {
			clearTimeout(this.pendingRefresh);
			this.pendingRefresh = undefined;
		}
		this.emitter.dispose();
		for (const subscription of this.subscriptions) {
			subscription.dispose();
		}
	}

	private rawTreeItem(node: WorkspaceNode): vscode.TreeItem {
		if (node.kind === "section") {
			return sectionItem(node);
		}
		switch (node.kind) {
			case "setup":
				return this.setup.getTreeItem(node.node);
			case "daemon":
				return this.daemons.getTreeItem(node.node);
			case "symbols":
				return this.symbols.getTreeItem(node.node);
			case "views":
				return this.views.getTreeItem(node.node);
			case "changes":
				return this.changes.getTreeItem(node.node);
			case "check":
				return this.rules.getTreeItem(node.node);
			case "ruleFiles":
				return this.ruleFiles.getTreeItem(node.node);
		}
	}

	private async sectionChildren(id: SectionId): Promise<WorkspaceNode[]> {
		switch (id) {
			case "setup":
				return wrapSetup(await this.setup.getChildren());
			case "daemon":
				return wrapDaemons(await this.daemons.getChildren());
			case "symbols":
				return wrapSymbols(await this.symbols.getChildren());
			case "views":
				return wrapViews(await this.views.getChildren());
			case "changes":
				return wrapChanges(await this.changes.getChildren());
			case "check":
				return wrapRules(await this.rules.getChildren(checkSection()));
			case "ruleFiles":
				return wrapRuleFiles(await this.ruleFiles.getChildren());
		}
	}

	private track(children: WorkspaceNode[], parent: WorkspaceNode | undefined): WorkspaceNode[] {
		for (const child of children) this.parents.set(child, parent);
		return children;
	}
}

function sections(): SectionNode[] {
	return [
		{ kind: "section", id: "setup", label: "Setup" },
		{ kind: "section", id: "daemon", label: "Daemon" },
		{ kind: "section", id: "symbols", label: "Symbols" },
		{ kind: "section", id: "views", label: "Views" },
		{ kind: "section", id: "changes", label: "Changes" },
		{ kind: "section", id: "check", label: "Check" },
		{ kind: "section", id: "ruleFiles", label: "Rule Files" },
	];
}

// Only the two sections that answer "is this workspace working" open by
// themselves. The content sections can each be thousands of rows, and opening
// all of them at once buries the answer the user came for.
const OPEN_SECTIONS = new Set<SectionId>(["setup", "daemon"]);

function sectionItem(node: SectionNode): vscode.TreeItem {
	const item = new vscode.TreeItem(
		node.label,
		OPEN_SECTIONS.has(node.id)
			? vscode.TreeItemCollapsibleState.Expanded
			: vscode.TreeItemCollapsibleState.Collapsed,
	);
	item.contextValue = `cmWorkspace.${node.id}`;
	item.iconPath = workspaceSectionIcon(node.id);
	return item;
}

function wrapSetup(nodes: SetupTreeNode[]): WorkspaceSetupNode[] {
	return nodes.map((node) => ({ kind: "setup", node }));
}

function wrapDaemons(nodes: DaemonNode[]): WorkspaceDaemonNode[] {
	return nodes.map((node) => ({ kind: "daemon", node }));
}

function wrapSymbols(nodes: SymbolTreeNode[]): WorkspaceSymbolNode[] {
	return nodes.map((node) => ({ kind: "symbols", node }));
}

function wrapViews(nodes: ViewTreeNode[]): WorkspaceViewNode[] {
	return nodes.map((node) => ({ kind: "views", node }));
}

function wrapChanges(nodes: ChangeTreeNode[]): WorkspaceChangeNode[] {
	return nodes.map((node) => ({ kind: "changes", node }));
}

function wrapRules(nodes: RulesTreeNode[]): WorkspaceRulesNode[] {
	return nodes.map((node) => ({ kind: "check", node }));
}

function wrapRuleFiles(nodes: RuleTreeNode[]): WorkspaceRuleFileNode[] {
	return nodes.map((node) => ({ kind: "ruleFiles", node }));
}

function checkSection(): RulesTreeNode {
	return { kind: "section", id: "check", label: "Check" };
}

function identityFromUri(uri: string): string {
	const scheme = uri.indexOf("://");
	if (scheme < 0) return uri;
	const rooted = uri.slice(scheme + 3);
	const separator = rooted.indexOf("/");
	return separator < 0 ? "" : rooted.slice(separator + 1);
}

function symbolNodeIdentity(node: SymbolTreeNode): string | undefined {
	if (node.kind === "identity") return node.row.identity;
	if (node.kind === "symbol") return node.identity;
	return undefined;
}
