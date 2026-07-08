import * as vscode from "vscode";

import { SymbolDto, UsageDto, UsageSummaryDto } from "../../daemon/model";
import { codeMonikerIcon } from "../../shared/appIcons";
import { SymbolRepository } from "../repository";
import { HighlightedSourceSnippet, highlightSource } from "./highlight";
import { renderDetailHtml } from "./html";

export interface SourceTarget {
	root: string;
	file: string;
	line: number;
}

export interface DetailPayload {
	symbol: SymbolDto;
	source: HighlightedSourceSnippet | null;
	incoming: HighlightedUsageDto[];
	outgoing: HighlightedUsageDto[];
	incomingSummary: UsageSummaryDto | null;
	outgoingSummary: UsageSummaryDto | null;
}

export interface HighlightedUsageDto extends UsageDto {
	snippet?: HighlightedSourceSnippet | null;
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
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		const renderedSymbol = detail?.symbol ?? symbol;
		const source = detail?.source
			? await highlightSource(detail.source, renderedSymbol.language)
			: null;
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		const incoming = usages?.rows.filter((row) => row.direction === "incoming") ?? [];
		const outgoing = usages?.rows.filter((row) => row.direction === "outgoing") ?? [];
		const payload: DetailPayload = {
			symbol: renderedSymbol,
			source,
			incoming,
			outgoing,
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
				localResourceRoots: [vscode.Uri.joinPath(this.extensionUri, "media")],
			},
		);
		panel.iconPath = codeMonikerIcon(this.extensionUri);
		panel.webview.html = renderDetailHtml(panel.webview, this.extensionUri);
		panel.onDidDispose(() => {
			this.panel = undefined;
		});
		panel.webview.onDidReceiveMessage((message: DetailWebviewMessage) => {
			if (message?.type === "openSource" && message.target) {
				void vscode.commands.executeCommand("codeMoniker.symbols.openSource", message.target);
			} else if (message?.type === "loadUsageSnippet") {
				void this.loadUsageSnippet(panel, message);
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

	private async loadUsageSnippet(
		panel: vscode.WebviewPanel,
		message: UsageSnippetRequest,
	): Promise<void> {
		const token = this.seq;
		const snippet = await usageSnippet(message.target);
		if (token !== this.seq || this.panel !== panel) {
			return;
		}
		await panel.webview.postMessage({
			type: "usageSnippet",
			requestId: message.requestId,
			snippet,
		});
	}
}

type DetailWebviewMessage =
	| { type?: "ready" }
	| { type: "openSource"; target: SourceTarget }
	| UsageSnippetRequest;

interface UsageSnippetRequest {
	type: "loadUsageSnippet";
	requestId: string;
	target: UsageDto;
}

async function usageSnippet(row: UsageDto): Promise<HighlightedSourceSnippet | null> {
	if (isImportUsage(row) || !row.line_range) {
		return null;
	}
	try {
		const snippet = await SymbolRepository.sourceSnippet(row, 4);
		if (!snippet) {
			return null;
		}
		return await highlightSource(snippet, "");
	} catch {
		return null;
	}
}

function isImportUsage(row: UsageDto): boolean {
	return row.kind.toLowerCase().startsWith("imports_");
}
