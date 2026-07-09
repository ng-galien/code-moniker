import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { SymbolDto } from "../daemon/model";
import { ExplorerPanel } from "./panel";
import { ExplorerRepository } from "./repository";

export interface ExplorerFeature {
	panel: ExplorerPanel;
}

export function registerExplorer(
	context: vscode.ExtensionContext,
	session: DaemonSession,
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

async function promptFocus(panel: ExplorerPanel): Promise<void> {
	const focus = await vscode.window.showInputBox({
		title: "Graph Explorer focus",
		prompt: "Symbol URI or workspace-relative file path",
	});
	if (focus) {
		void panel.focus(focus);
	}
}
