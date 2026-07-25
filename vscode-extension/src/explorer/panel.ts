import * as vscode from "vscode";

import { IdentityGraphResult } from "../daemon/model";
import { highlightSource } from "../symbols/detail/highlight";
import { renderExplorerHtml } from "./html";
import { parentPrefix, segmentName } from "../shared/identity";
import {
	ExplorerMessage,
	InsetAck,
	InsetMessage,
	ScopeAck,
	ScopeErrorMessage,
	ScopeMessage,
	ScopeOutline,
	ScopePayload,
} from "./protocol";
import { ExplorerRepository } from "./repository";

// The scoped exploration graph: an editor-area webview where the focus is an
// identity prefix. Nodes are the scope's children, edges are rolled-up
// references, ports cross the boundary. Diving pushes a deeper prefix;
// history supports walking back and forward.

const MEMBER_PREVIEW = 6;

export class ExplorerPanel implements vscode.Disposable {
	private panel?: vscode.WebviewPanel;
	private history: string[] = [];
	private index = -1;
	private seq = 0;
	private lastMessage?: ScopeMessage | ScopeErrorMessage;
	private acks: ScopeAck[] = [];
	private insetAckList: InsetAck[] = [];

	// Test observability: the last message the host decided to show, and the
	// acks the webview sent back after actually rendering scopes and insets.
	get current(): ScopeMessage | ScopeErrorMessage | undefined {
		return this.lastMessage;
	}

	get webviewAcks(): readonly ScopeAck[] {
		return this.acks;
	}

	get insetAcks(): readonly InsetAck[] {
		return this.insetAckList;
	}

	// Same path as the webview's inspect message; lets the e2e suite drive
	// the inset flow, since the harness cannot click inside a webview.
	async inspect(uri: string): Promise<void> {
		if (this.panel) {
			await this.sendInset(this.panel, uri);
		}
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
		// Single-child chains carry no information: dive through them (cheap
		// children walk) so one gesture lands on the first real branching
		// point, then pay for exactly one graph rollup.
		const landing = await this.repository.collapsedChain(prefix);
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		// A leaf scope (a plain function) has nothing to draw: focus the parent
		// so the leaf appears as a node among its siblings. The chain walk
		// already listed the landing, so the empty case costs no rollup.
		const target =
			landing.children.length === 0 && landing.identity.includes("/")
				? parentPrefix(landing.identity)
				: landing.identity;
		this.history[this.index] = target;
		const graph = await this.repository.scopeGraph(target);
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		if (!graph) {
			throw new Error(
				"the daemon returned no scope graph — reconnect or refresh the workspace daemon",
			);
		}
		// Outlines decide how tall each card is, so the webview needs them in
		// the same message: laying out first and resizing after would run ELK
		// twice and remount the canvas.
		const outline = await this.outlineFor(graph);
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		panel.title = scopeTitle(graph.prefix);
		const payload: ScopePayload = {
			graph,
			canBack: this.index > 0,
			canForward: this.index < this.history.length - 1,
			outline,
		};
		this.lastMessage = { type: "scope", payload };
		void panel.webview.postMessage(this.lastMessage);
	}

	// What each container holds, so its card shows its members instead of
	// making the user dive to find out: the flattened single-child path plus
	// a preview of the members at the landing. One concurrent round-trip —
	// the listings are cached per generation and shared with the symbol tree.
	private async outlineFor(graph: IdentityGraphResult): Promise<ScopeOutline> {
		const outline: ScopeOutline = {};
		const containers = graph.nodes.filter((node) => !node.symbol && node.has_children);
		await Promise.all(
			containers.map(async (node) => {
				const chain = await this.repository.collapsedChain(node.identity);
				outline[node.identity] = {
					chain: chain.names,
					members: chain.children.slice(0, MEMBER_PREVIEW).map((child) => ({
						identity: child.identity,
						name: child.name,
						kind: child.symbol?.kind ?? child.kind,
					})),
					hidden: Math.max(0, chain.children.length - MEMBER_PREVIEW),
				};
			}),
		);
		return outline;
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
		panel.iconPath = {
			light: vscode.Uri.joinPath(this.extensionUri, "icons", "graph-light.svg"),
			dark: vscode.Uri.joinPath(this.extensionUri, "icons", "graph-dark.svg"),
		};
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
			} else if (message?.type === "insetAck") {
				this.insetAckList.push({ uri: message.uri, lines: message.lines });
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
	return prefix ? segmentName(prefix) : "Graph Explorer";
}
