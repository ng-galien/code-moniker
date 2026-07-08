import * as path from "node:path";
import * as vscode from "vscode";

import { daemonIcon } from "../shared/appIcons";
import { DaemonNode } from "./nodes";
import { entryMatchesRoots, listDaemons } from "./registry";
import { DaemonSession } from "./session";

// Lists every running daemon recorded in the shared registry, flagging the one
// that serves the currently-open workspace.
export class DaemonListProvider implements vscode.TreeDataProvider<DaemonNode> {
	private readonly emitter = new vscode.EventEmitter<DaemonNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;

	constructor(
		private readonly session: DaemonSession,
		private readonly roots: string[],
	) {}

	refresh(): void {
		this.emitter.fire(undefined);
	}

	getChildren(node?: DaemonNode): DaemonNode[] {
		if (node) {
			return [];
		}
		return listDaemons().map((entry) => ({
			entry,
			current: entryMatchesRoots(entry, this.roots),
		}));
	}

	getTreeItem(node: DaemonNode): vscode.TreeItem {
		const item = new vscode.TreeItem(
			path.basename(node.entry.workspace_root) || node.entry.workspace_root,
			vscode.TreeItemCollapsibleState.None,
		);
		item.description = this.description(node);
		item.iconPath = this.icon(node);
		item.contextValue = node.current ? "cmDaemonCurrent" : "cmDaemon";
		item.tooltip = this.tooltip(node);
		item.resourceUri = vscode.Uri.file(node.entry.workspace_root);
		return item;
	}

	private description(node: DaemonNode): string {
		const parts = [`pid ${node.entry.pid}`, node.entry.endpoint];
		if (node.current) {
			parts.push(this.session.status);
		}
		return parts.join(" · ");
	}

	private icon(node: DaemonNode): vscode.ThemeIcon {
		if (!node.current) {
			return daemonIcon();
		}
		return daemonIcon(this.session.status);
	}

	private tooltip(node: DaemonNode): vscode.MarkdownString {
		const md = new vscode.MarkdownString();
		md.appendMarkdown(`**${node.entry.workspace_root}**\n\n`);
		md.appendMarkdown(`- endpoint: \`${node.entry.endpoint}\`\n`);
		md.appendMarkdown(`- pid: ${node.entry.pid}\n`);
		if (node.entry.live_refresh) {
			md.appendMarkdown(`- live refresh: ${node.entry.live_refresh}\n`);
		}
		for (const root of node.entry.workspace_roots) {
			md.appendMarkdown(`- root: \`${root}\`\n`);
		}
		if (node.current) {
			md.appendMarkdown(`\nServes this window · status: **${this.session.status}**`);
			if (this.session.lastError) {
				md.appendMarkdown(`\n\n${this.session.lastError}`);
			}
		}
		return md;
	}
}
