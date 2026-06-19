import * as vscode from "vscode";

import { SourceSnippet, SymbolDto, UsageDto, UsageSummaryDto } from "../../daemon/model";
import { SymbolRepository } from "../repository";
import { renderDetailHtml } from "./html";

export interface SourceTarget {
	root: string;
	file: string;
	line: number;
}

export interface DetailPayload {
	symbol: SymbolDto;
	source: SourceSnippet | null;
	incoming: UsageDto[];
	outgoing: UsageDto[];
	incomingSummary: UsageSummaryDto | null;
	outgoingSummary: UsageSummaryDto | null;
}

export interface DetailDocument {
	title: string;
	kind: string;
	description?: string;
	meta?: DetailRow[];
	sections?: DetailSection[];
}

export interface DetailSection {
	title: string;
	rows?: DetailRow[];
	text?: string;
}

export interface DetailRow {
	label: string;
	value: string;
}

// A single reactive webview that mirrors the TUI's right-hand panel: it re-renders
// from the selected symbol without ever opening a file. Opening source is an
// explicit message from the view.
export class DetailWebview implements vscode.Disposable {
	private panel?: vscode.WebviewPanel;
	private seq = 0;
	private rendered?: DetailPayload;
	private renderedDocument?: DetailDocument;
	private lastMessage?: { type: "detail"; payload: DetailPayload } | { type: "document"; payload: DetailDocument };

	constructor(
		private readonly extensionUri: vscode.Uri,
		private readonly repository: SymbolRepository,
	) {}

	// Test introspection: the last payload posted to the webview and whether the
	// panel is live. The webview DOM itself is rendered from `lastPayload`.
	get lastPayload(): DetailPayload | undefined {
		return this.rendered;
	}

	get lastDocument(): DetailDocument | undefined {
		return this.renderedDocument;
	}

	get visible(): boolean {
		return this.panel?.visible ?? false;
	}

	async showForSymbol(symbol: SymbolDto): Promise<void> {
		const token = ++this.seq;
		const panel = this.ensurePanel();
		panel.title = symbol.name;
		const [detail, usages] = await Promise.all([
			this.repository.symbolDetail(symbol.uri),
			this.repository.symbolUsages(symbol.uri),
		]);
		if (token !== this.seq || !this.panel) {
			return; // a newer selection won; drop this stale render
		}
		const payload: DetailPayload = {
			symbol: detail?.symbol ?? symbol,
			source: detail?.source ?? null,
			incoming: usages?.rows.filter((row) => row.direction === "incoming") ?? [],
			outgoing: usages?.rows.filter((row) => row.direction === "outgoing") ?? [],
			incomingSummary: usages?.incoming_summary ?? null,
			outgoingSummary: usages?.outgoing_summary ?? null,
		};
		this.rendered = payload;
		this.renderedDocument = undefined;
		this.post(panel, { type: "detail", payload });
	}

	showDocument(document: DetailDocument): void {
		const panel = this.ensurePanel();
		this.seq++;
		panel.title = document.title;
		this.renderedDocument = document;
		this.rendered = undefined;
		this.post(panel, { type: "document", payload: document });
	}

	private post(
		panel: vscode.WebviewPanel,
		message: { type: "detail"; payload: DetailPayload } | { type: "document"; payload: DetailDocument },
	): void {
		this.lastMessage = message;
		void panel.webview.postMessage(message);
	}

	private ensurePanel(): vscode.WebviewPanel {
		if (this.panel) {
			return this.panel;
		}
		const panel = vscode.window.createWebviewPanel(
			"codeMoniker.symbolDetail",
			"Symbol",
			{ viewColumn: vscode.ViewColumn.Beside, preserveFocus: true },
			{
				enableScripts: true,
				retainContextWhenHidden: true,
				localResourceRoots: [vscode.Uri.joinPath(this.extensionUri, "media")],
			},
		);
		panel.webview.html = renderDetailHtml(panel.webview, this.extensionUri);
		panel.onDidDispose(() => {
			this.panel = undefined;
		});
		panel.webview.onDidReceiveMessage((message: { type?: string; target?: SourceTarget }) => {
			if (message?.type === "openSource" && message.target) {
				void vscode.commands.executeCommand("codeMoniker.symbols.openSource", message.target);
			} else if (message?.type === "ready" && this.lastMessage) {
				void panel.webview.postMessage(this.lastMessage);
			}
		});
		this.panel = panel;
		return panel;
	}

	dispose(): void {
		this.panel?.dispose();
	}
}
