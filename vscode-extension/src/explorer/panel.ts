import * as vscode from "vscode";

import { renderExplorerHtml } from "./html";
import { ExplorerMessage, ScopeMessage, ScopePayload } from "./protocol";
import { ExplorerRepository } from "./repository";

// The scoped exploration graph: an editor-area webview where the focus is an
// identity prefix. Nodes are the scope's children, edges are rolled-up
// references, ports cross the boundary. Diving pushes a deeper prefix;
// history supports walking back and forward.
export class ExplorerPanel implements vscode.Disposable {
	private panel?: vscode.WebviewPanel;
	private history: string[] = [];
	private index = -1;
	private seq = 0;
	private lastMessage?: ScopeMessage;

	constructor(
		private readonly extensionUri: vscode.Uri,
		private readonly repository: ExplorerRepository,
	) {}

	async focus(prefix: string): Promise<void> {
		this.pushHistory(prefix);
		await this.show(prefix);
	}

	async refreshCurrent(): Promise<void> {
		const current = this.history[this.index];
		if (current !== undefined && this.panel) {
			await this.show(current);
		}
	}

	dispose(): void {
		this.panel?.dispose();
	}

	private pushHistory(prefix: string): void {
		if (this.history[this.index] === prefix) {
			return;
		}
		this.history.splice(this.index + 1);
		this.history.push(prefix);
		this.index = this.history.length - 1;
	}

	private async show(prefix: string): Promise<void> {
		const token = ++this.seq;
		const panel = this.ensurePanel();
		let graph = await this.repository.scopeGraph(prefix);
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		// A leaf scope (a plain function) has nothing to draw: climb to the
		// parent level so the leaf appears as a node among its siblings.
		if (graph && graph.nodes.length === 0 && graph.prefix.includes("/")) {
			const parent = graph.prefix.slice(0, graph.prefix.lastIndexOf("/"));
			this.history[this.index] = parent;
			graph = await this.repository.scopeGraph(parent);
			if (token !== this.seq || this.panel !== panel) {
				return;
			}
		}
		if (!graph) {
			return;
		}
		panel.title = scopeTitle(graph.prefix);
		const payload: ScopePayload = {
			graph,
			canBack: this.index > 0,
			canForward: this.index < this.history.length - 1,
		};
		this.lastMessage = { type: "scope", payload };
		void panel.webview.postMessage(this.lastMessage);
	}

	private ensurePanel(): vscode.WebviewPanel {
		if (this.panel) {
			this.panel.reveal(undefined, true);
			return this.panel;
		}
		const panel = vscode.window.createWebviewPanel(
			"codeMoniker.graphExplorer",
			"Graph Explorer",
			{ viewColumn: vscode.ViewColumn.Active, preserveFocus: false },
			{
				enableScripts: true,
				localResourceRoots: [vscode.Uri.joinPath(this.extensionUri, "media")],
			},
		);
		panel.webview.html = renderExplorerHtml(panel.webview, this.extensionUri);
		panel.onDidDispose(() => {
			this.panel = undefined;
		});
		panel.webview.onDidReceiveMessage((message: ExplorerMessage) => {
			if (message?.type === "focus" && message.prefix !== undefined) {
				void this.focus(message.prefix);
			} else if (message?.type === "back") {
				void this.step(-1);
			} else if (message?.type === "forward") {
				void this.step(1);
			} else if (message?.type === "openSource" && message.target) {
				void vscode.commands.executeCommand("codeMoniker.symbols.openSource", message.target);
			} else if (message?.type === "ready" && this.lastMessage) {
				void panel.webview.postMessage(this.lastMessage);
			}
		});
		this.panel = panel;
		return panel;
	}

	private async step(delta: number): Promise<void> {
		const next = this.index + delta;
		if (next < 0 || next >= this.history.length) {
			return;
		}
		this.index = next;
		await this.show(this.history[this.index]);
	}
}

function scopeTitle(prefix: string): string {
	if (!prefix) {
		return "Graph Explorer";
	}
	const segment = prefix.split("/").pop() ?? prefix;
	return segment.split(":")[1] ?? segment;
}
