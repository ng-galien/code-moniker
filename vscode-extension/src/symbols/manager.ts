import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { registerSymbolCommands } from "./commands";
import { DetailWebview } from "./detail/panel";
import { SymbolRepository } from "./repository";
import { SymbolTreeProvider } from "./tree";

export interface SymbolFeature {
	tree: SymbolTreeProvider;
	detail: DetailWebview;
}

// Wires the symbol tree to the shared session and drives the detail webview from
// tree selection (no editor open). Returns the tree so the rules feature can
// overlay violation decorations.
export function registerSymbols(
	context: vscode.ExtensionContext,
	session: DaemonSession,
): SymbolFeature {
	const repository = new SymbolRepository(session);
	const provider = new SymbolTreeProvider(session, repository);
	const detail = new DetailWebview(context.extensionUri, repository);
	const treeView = vscode.window.createTreeView("codeMoniker.symbols", {
		treeDataProvider: provider,
		showCollapseAll: true,
	});

	context.subscriptions.push(
		treeView,
		detail,
		treeView.onDidChangeSelection((event) => {
			const selected = event.selection[0];
			if (selected?.kind === "symbol") {
				void detail.showForSymbol(selected.symbol);
			}
		}),
		session.onDidChangeStatus(() => provider.refresh()),
	);

	registerSymbolCommands(context, provider, detail);

	return { tree: provider, detail };
}
