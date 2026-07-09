import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { DaemonListProvider } from "../daemon/tree";
import { RuleFilesProvider } from "../rules/tree";
import { RulesProvider } from "../rules-daemon/tree";
import { DetailWebview } from "../symbols/detail/panel";
import { SymbolTreeProvider } from "../symbols/tree";
import { ViewsProvider } from "../views/tree";
import { renderWorkspaceNode } from "./render";
import { WorkspaceTreeProvider } from "./workspaceTree";

export interface WorkspaceFeature {
	tree: WorkspaceTreeProvider;
}

export interface WorkspaceInputs {
	session: DaemonSession;
	daemons: DaemonListProvider;
	symbols: SymbolTreeProvider;
	views: ViewsProvider;
	detail: DetailWebview;
	rules: RulesProvider;
	ruleFiles: RuleFilesProvider;
}

export function registerWorkspace(
	context: vscode.ExtensionContext,
	inputs: WorkspaceInputs,
): WorkspaceFeature {
	const provider = new WorkspaceTreeProvider(
		inputs.daemons,
		inputs.symbols,
		inputs.views,
		inputs.rules,
		inputs.ruleFiles,
	);
	const treeView = vscode.window.createTreeView("codeMoniker.workspace", {
		treeDataProvider: provider,
		showCollapseAll: true,
	});

	let pendingSelection: NodeJS.Timeout | undefined;
	const SELECTION_DEBOUNCE_MS = 180;

	context.subscriptions.push(
		provider,
		treeView,
		treeView.onDidChangeSelection((event) => {
			const node = event.selection[0];
			if (pendingSelection) {
				clearTimeout(pendingSelection);
				pendingSelection = undefined;
			}
			if (node?.kind === "symbols" && node.node.kind === "symbol") {
				const symbol = node.node.symbol;
				pendingSelection = setTimeout(() => {
					pendingSelection = undefined;
					void inputs.detail.showForSymbol(symbol);
				}, SELECTION_DEBOUNCE_MS);
				return;
			}
			if (node) {
				const document = renderWorkspaceNode(node);
				if (document) {
					inputs.detail.showDocument(document);
				}
			}
		}),
		new vscode.Disposable(() => {
			if (pendingSelection) {
				clearTimeout(pendingSelection);
			}
		}),
		inputs.session.onWorkspaceEvent((event) => {
			if (event.kind === "stale" || event.kind === "refreshed") {
				inputs.daemons.refresh();
				inputs.symbols.refresh();
				inputs.views.refresh();
				inputs.rules.refresh();
			}
		}),
	);

	return { tree: provider };
}
