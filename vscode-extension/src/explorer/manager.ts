import * as path from "node:path";
import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { SymbolDto } from "../daemon/model";
import { SymbolRepository } from "../symbols/repository";
import { ExplorerPanel } from "./panel";
import { ExplorerRepository } from "./repository";

export interface ExplorerFeature {
	panel: ExplorerPanel;
}

export function registerExplorer(
	context: vscode.ExtensionContext,
	session: DaemonSession,
	symbols: SymbolRepository,
): ExplorerFeature {
	const repository = new ExplorerRepository(session);
	const panel = new ExplorerPanel(context.extensionUri, repository);

	context.subscriptions.push(
		panel,
		vscode.commands.registerCommand("codeMoniker.explorer.focus", (arg?: unknown) => {
			const focus = focusFromArgument(arg);
			if (focus) {
				void panel.focus(focus);
			} else {
				void promptFocus(panel);
			}
		}),
		vscode.commands.registerCommand("codeMoniker.explorer.focusAtCursor", () =>
			focusAtCursor(session, symbols, panel),
		),
		session.onWorkspaceEvent((event) => {
			if (event.kind === "refreshed") {
				void panel.refreshCurrent();
			}
		}),
	);

	return { panel };
}

function focusFromArgument(arg: unknown): string | undefined {
	if (typeof arg === "string") {
		return arg;
	}
	const symbol = symbolFromNode(arg);
	return symbol?.uri;
}

function symbolFromNode(arg: unknown): SymbolDto | undefined {
	if (!arg || typeof arg !== "object") {
		return undefined;
	}
	let node = arg as { kind?: string; node?: unknown; symbol?: SymbolDto };
	if (node.kind === "symbols" && node.node) {
		node = node.node as { kind?: string; symbol?: SymbolDto };
	}
	return node.kind === "symbol" ? node.symbol : undefined;
}

async function focusAtCursor(
	session: DaemonSession,
	symbols: SymbolRepository,
	panel: ExplorerPanel,
): Promise<void> {
	const editor = vscode.window.activeTextEditor;
	if (!editor) {
		return;
	}
	const rel = workspaceRelative(session, editor.document.uri.fsPath);
	if (!rel) {
		void vscode.window.showInformationMessage(
			"Code Moniker: the active file is outside the daemon workspace.",
		);
		return;
	}
	const line = editor.selection.active.line + 1;
	const nodes = await symbols.fileSymbols(rel);
	const uri = tightestSymbolAt(nodes, line);
	await panel.focus(uri ?? rel);
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

function tightestSymbolAt(nodes: unknown[], line: number): string | undefined {
	let best: { uri: string; span: number } | undefined;
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
					best = { uri: node.symbol.uri, span };
				}
			}
			if (node.children) {
				visit(node.children);
			}
		}
	};
	visit(nodes);
	return best?.uri;
}

async function promptFocus(panel: ExplorerPanel): Promise<void> {
	const focus = await vscode.window.showInputBox({
		title: "Graph Explorer focus",
		prompt: "Symbol URI or workspace-relative file path",
	});
	if (focus) {
		void panel.focus(focus);
	}
}
