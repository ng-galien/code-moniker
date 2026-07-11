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
		vscode.commands.registerCommand("codeMoniker.explorer.focus", async (arg?: unknown) => {
			if (!(await ensureGraphCapable(session))) {
				return;
			}
			const focus = focusFromArgument(arg);
			if (focus !== undefined) {
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

// A long-running daemon may predate the graph verb while reporting the same
// version string; the handshake capability set is the only honest signal.
// Offer the restart instead of letting the query fail with a wire error.
async function ensureGraphCapable(session: DaemonSession): Promise<boolean> {
	if (!(await session.connectOrStart())) {
		void vscode.window.showWarningMessage("Code Moniker: no workspace daemon available.");
		return false;
	}
	if (session.supportsQuery("identity.graph")) {
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
	if (!(await session.connectOrStart()) || !session.supportsQuery("identity.graph")) {
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
	if (node.kind === "identity") {
		return node.row?.identity;
	}
	if (node.kind === "symbol") {
		return node.identity ?? node.symbol?.uri;
	}
	return undefined;
}

async function focusAtCursor(
	session: DaemonSession,
	symbols: SymbolRepository,
	panel: ExplorerPanel,
): Promise<void> {
	const editor = vscode.window.activeTextEditor;
	if (!editor || !(await ensureGraphCapable(session))) {
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
	await panel.focus(uri ?? "");
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
