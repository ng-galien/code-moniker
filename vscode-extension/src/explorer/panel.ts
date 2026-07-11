import * as vscode from "vscode";

import { highlightSource } from "../symbols/detail/highlight";
import { renderExplorerHtml } from "./html";
import {
	ExplorerMessage,
	InsetMessage,
	ScopeAck,
	ScopeErrorMessage,
	ScopeMessage,
	ScopePayload,
} from "./protocol";
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
	private lastMessage?: ScopeMessage | ScopeErrorMessage;
	private acks: ScopeAck[] = [];

	// Test observability: the last message the host decided to show, and the
	// acks the webview sent back after actually applying a scope.
	get current(): ScopeMessage | ScopeErrorMessage | undefined {
		return this.lastMessage;
	}

	get webviewAcks(): readonly ScopeAck[] {
		return this.acks;
	}

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
		try {
			await this.render(panel, token, prefix);
		} catch (error) {
			if (token === this.seq && this.panel === panel) {
				// Store the error like a scope: if it fires before the webview
				// has mounted, the ready handshake replays it instead of
				// leaving the user on a silent empty state.
				this.lastMessage = {
					type: "scopeError",
					prefix,
					message: error instanceof Error ? error.message : String(error),
				};
				void panel.webview.postMessage(this.lastMessage);
			}
		}
	}

	private async render(
		panel: vscode.WebviewPanel,
		token: number,
		prefix: string,
	): Promise<void> {
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
			throw new Error(
				"the daemon returned no scope graph — reconnect or refresh the workspace daemon",
			);
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
			} else if (message?.type === "inspect" && message.uri) {
				void this.sendInset(panel, message.uri);
			} else if (message?.type === "openSource" && message.target) {
				void vscode.commands.executeCommand("codeMoniker.symbols.openSource", message.target);
			} else if (message?.type === "ready" && this.lastMessage) {
				void panel.webview.postMessage(this.lastMessage);
			} else if (message?.type === "ack") {
				this.acks.push({ prefix: message.prefix, nodes: message.nodes });
			}
		});
		this.panel = panel;
		return panel;
	}

	// The code zone of one definition, highlighted host-side. Failures fall
	// back to a null source; the webview says so instead of staying silent.
	private async sendInset(panel: vscode.WebviewPanel, uri: string): Promise<void> {
		try {
			const detail = await this.repository.symbolDetail(uri);
			if (this.panel !== panel || !detail?.symbol) {
				return;
			}
			const source = detail.source
				? await highlightSource(detail.source, detail.symbol.language)
				: null;
			if (this.panel !== panel) {
				return;
			}
			const message: InsetMessage = { type: "inset", uri, symbol: detail.symbol, source };
			void panel.webview.postMessage(message);
		} catch {
			void panel.webview.postMessage({ type: "inset", uri, symbol: null, source: null });
		}
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
