import * as vscode from "vscode";

import {
	SymbolDto,
	SymbolGraphEdge,
	SymbolGraphFocus,
	SymbolGraphNeighbor,
} from "../daemon/model";
import { HighlightedSourceSnippet, highlightSource } from "../symbols/detail/highlight";
import { renderExplorerHtml } from "./html";
import { ExplorerRepository } from "./repository";

// The ego-centric graph explorer: an editor-area webview showing the focused
// functional unit as a triptych - callers on the left, the unit (header +
// code) in the center, external callees on the right. Clicking a neighbor
// translates the focus; history supports walking back and forward. Facts are
// shown verbatim: relation kinds, call counts, recursion, unresolved refs.
export interface UnitPayload {
	focus: SymbolGraphFocus;
	members: SymbolDto[];
	internalEdges: SymbolGraphEdge[];
	callers: SymbolGraphNeighbor[];
	callees: SymbolGraphNeighbor[];
	unresolvedRefs: number;
	source: HighlightedSourceSnippet | null;
	canBack: boolean;
	canForward: boolean;
}

interface ExplorerMessage {
	type: "focus" | "back" | "forward" | "openSource" | "ready";
	uri?: string;
	target?: { root: string; file: string; line: number };
}

export class ExplorerPanel implements vscode.Disposable {
	private panel?: vscode.WebviewPanel;
	private history: string[] = [];
	private index = -1;
	private seq = 0;
	private lastMessage?: { type: "unit"; payload: UnitPayload };

	constructor(
		private readonly extensionUri: vscode.Uri,
		private readonly repository: ExplorerRepository,
	) {}

	async focus(focus: string): Promise<void> {
		this.pushHistory(focus);
		await this.show(focus);
	}

	async refreshCurrent(): Promise<void> {
		const current = this.history[this.index];
		if (current && this.panel) {
			await this.show(current);
		}
	}

	dispose(): void {
		this.panel?.dispose();
	}

	private pushHistory(focus: string): void {
		if (this.history[this.index] === focus) {
			return;
		}
		this.history.splice(this.index + 1);
		this.history.push(focus);
		this.index = this.history.length - 1;
	}

	private async step(delta: number): Promise<void> {
		const next = this.index + delta;
		if (next < 0 || next >= this.history.length) {
			return;
		}
		this.index = next;
		await this.show(this.history[this.index]);
	}

	private async show(focus: string): Promise<void> {
		const token = ++this.seq;
		const panel = this.ensurePanel();
		const graph = await this.repository.unitGraph(focus);
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		if (!graph) {
			return;
		}
		let source: HighlightedSourceSnippet | null = null;
		let title = "Graph Explorer";
		if (graph.focus.kind === "symbol") {
			title = graph.focus.symbol.name;
			const detail = await this.repository.symbolDetail(graph.focus.symbol.uri);
			if (token !== this.seq || this.panel !== panel) {
				return;
			}
			source = detail?.source
				? await highlightSource(detail.source, graph.focus.symbol.language)
				: null;
			if (token !== this.seq || this.panel !== panel) {
				return;
			}
		} else {
			title = graph.focus.path;
		}
		panel.title = title;
		const payload: UnitPayload = {
			focus: graph.focus,
			members: graph.members,
			internalEdges: graph.internal_edges,
			callers: graph.callers,
			callees: graph.callees,
			unresolvedRefs: graph.unresolved_refs,
			source,
			canBack: this.index > 0,
			canForward: this.index < this.history.length - 1,
		};
		this.lastMessage = { type: "unit", payload };
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
			if (message?.type === "focus" && message.uri) {
				void this.focus(message.uri);
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
}
