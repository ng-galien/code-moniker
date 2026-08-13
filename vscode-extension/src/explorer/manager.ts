import * as path from "node:path";
import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { SymbolDto } from "../daemon/model";
import { SymbolRepository } from "../symbols/repository";
import { WorkspaceFeature } from "../workbench/manager";
import { LatestRequest } from "../shared/latestRequest";
import { ExplorerPanel } from "./panel";
import { ExplorerRepository } from "./repository";

export interface ExplorerFeature {
	panel: ExplorerPanel;
}

export function registerExplorer(
	context: vscode.ExtensionContext,
	session: DaemonSession,
	symbols: SymbolRepository,
	workspace: WorkspaceFeature,
): ExplorerFeature {
	const repository = new ExplorerRepository(session);
	const panel = new ExplorerPanel(
		context.extensionUri,
		repository,
		context.workspaceState,
		(symbol) => workspace.revealSymbol(symbol),
	);
	let pendingEditorSelection: NodeJS.Timeout | undefined;
	let lastEditorSymbolKey: string | undefined;
	const editorRequests = new LatestRequest();
	const cancelPendingEditorSelection = () => {
		if (pendingEditorSelection) {
			clearTimeout(pendingEditorSelection);
			pendingEditorSelection = undefined;
		}
	};
	const focusActiveEditor = async (): Promise<void> => {
		cancelPendingEditorSelection();
		const token = editorRequests.begin();
		const editor = vscode.window.activeTextEditor;
		if (!editor || !(await ensureGraphCapable(session)) || !editorRequests.isCurrent(token)) return;
		if (!workspaceRelative(session, editor.document.uri.fsPath)) {
			void vscode.window.showInformationMessage(
				"Code Moniker: the active file is outside the daemon workspace.",
			);
			return;
		}
		const position = editor.selection.active;
		const symbol = await symbolAtEditor(session, symbols, editor, position);
		if (!editorRequests.isCurrent(token) || vscode.window.activeTextEditor !== editor) return;
		if (symbol) {
			lastEditorSymbolKey = `${editor.document.uri.toString()}\0${symbol.uri}`;
			await panel.focus(symbol.uri, "editor");
		} else {
			lastEditorSymbolKey = undefined;
			await panel.open();
		}
	};

	context.subscriptions.push(
		panel,
		vscode.commands.registerCommand("codeMoniker.explorer.open", async () => {
			if (await ensureGraphCapable(session)) {
				await panel.open();
			}
		}),
		// The focus command belongs to concrete symbol rows. Container identities
		// deliberately open no second graph mode: the cockpit is symbol-centered.
		vscode.commands.registerCommand("codeMoniker.explorer.focus", async (arg?: unknown) => {
			if (!(await ensureGraphCapable(session))) {
				return;
			}
			const focus = focusFromArgument(arg);
			if (focus) {
				await panel.focus(focus, "tree");
			} else {
				await panel.open();
			}
		}),
		vscode.commands.registerCommand("codeMoniker.explorer.focusAtCursor", focusActiveEditor),
		workspace.onDidSelectSymbol((symbol) => {
			if (panel.isOpen) void panel.focus(symbol.uri, "tree");
		}),
		workspace.onDidSelectSymbolContext((context) => {
			if (panel.isOpen) panel.openContext(context);
		}),
		vscode.window.onDidChangeTextEditorSelection((event) => {
			if (!panel.isOpen) return;
			cancelPendingEditorSelection();
			const token = editorRequests.begin();
			const position = event.selections[0]?.active ?? event.textEditor.selection.active;
			pendingEditorSelection = setTimeout(() => {
				pendingEditorSelection = undefined;
				void symbolAtEditor(session, symbols, event.textEditor, position).then((symbol) => {
					if (!editorRequests.isCurrent(token) || !panel.isOpen) return;
					if (!symbol) {
						lastEditorSymbolKey = undefined;
						return;
					}
					const key = `${event.textEditor.document.uri.toString()}\0${symbol.uri}`;
					if (lastEditorSymbolKey === key) return;
					lastEditorSymbolKey = key;
					panel.select(symbol, "editor");
					void workspace.revealSymbol(symbol);
				});
			}, 180);
		}),
		new vscode.Disposable(() => {
			cancelPendingEditorSelection();
			editorRequests.invalidate();
		}),
		session.onWorkspaceEvent((event) => {
			if (event.kind === "refreshed") {
				void panel.refreshCurrent();
			}
		}),
	);

	return { panel };
}

// A long-running daemon may predate the graph verb while reporting the same
// version string; the handshake capability set is the only honest signal.
// Offer the restart instead of letting the query fail with a wire error.
async function ensureGraphCapable(session: DaemonSession): Promise<boolean> {
	if (!(await session.connectOrStart())) {
		void vscode.window.showWarningMessage("Code Moniker: no workspace daemon available.");
		return false;
	}
	if (session.supportsQuery("symbol.graph")) {
		return true;
	}
	const restart = "Restart daemon";
	const choice = await vscode.window.showWarningMessage(
		"Code Moniker: the running workspace daemon predates the graph view.",
		restart,
	);
	if (choice !== restart) {
		return false;
	}
	await session.stop();
	if (!(await session.connectOrStart()) || !session.supportsQuery("symbol.graph")) {
		void vscode.window.showWarningMessage(
			"Code Moniker: the restarted daemon still lacks the graph view — update the code-moniker binary.",
		);
		return false;
	}
	return true;
}

// Accepts a raw prefix/URI string, a symbol row, or an identity segment row
// from the workspace tree; the daemon normalizes full URIs to identity paths.
function focusFromArgument(arg: unknown): string | undefined {
	if (typeof arg === "string") {
		return arg;
	}
	if (!arg || typeof arg !== "object") {
		return undefined;
	}
	let node = arg as {
		kind?: string;
		node?: unknown;
		symbol?: SymbolDto;
		identity?: string;
		row?: { identity?: string };
	};
	if (node.kind === "symbols" && node.node) {
		node = node.node as typeof node;
	}
	if (node.kind === "symbol") {
		return node.symbol?.uri;
	}
	return undefined;
}

async function symbolAtEditor(
	session: DaemonSession,
	symbols: SymbolRepository,
	editor: vscode.TextEditor,
	position: vscode.Position,
): Promise<SymbolDto | undefined> {
	const rel = workspaceRelative(session, editor.document.uri.fsPath);
	if (!rel) return undefined;
	const line = position.line + 1;
	return tightestSymbolAt(await symbols.fileSymbols(rel), line);
}

function workspaceRelative(session: DaemonSession, fsPath: string): string | undefined {
	for (const root of session.workspaceRoots) {
		const relative = path.relative(root, fsPath);
		if (relative && !relative.startsWith("..") && !path.isAbsolute(relative)) {
			return relative.split(path.sep).join("/");
		}
	}
	return undefined;
}

function tightestSymbolAt(nodes: unknown[], line: number): SymbolDto | undefined {
	let best: { symbol: SymbolDto; span: number } | undefined;
	const visit = (list: unknown[]) => {
		for (const raw of list) {
			const node = raw as {
				kind?: string;
				symbol?: SymbolDto;
				children?: unknown[];
			};
			if (node.kind !== "symbol" || !node.symbol) {
				continue;
			}
			const range = node.symbol.line_range;
			if (range && range[0] <= line && line <= range[1]) {
				const span = range[1] - range[0];
				if (!best || span < best.span) {
					best = { symbol: node.symbol, span };
				}
			}
			if (node.children) {
				visit(node.children);
			}
		}
	};
	visit(nodes);
	return best?.symbol;
}
