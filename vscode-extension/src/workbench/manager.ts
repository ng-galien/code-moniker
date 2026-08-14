import * as vscode from "vscode";

import { ChangesProvider } from "../changes/tree";
import { SymbolDto } from "../daemon/model";
import { DaemonSession } from "../daemon/session";
import { DaemonListProvider } from "../daemon/tree";
import { RuleFilesProvider } from "../rules/tree";
import { RulesProvider } from "../rules-daemon/tree";
import { SetupProvider } from "../setup/tree";
import { DetailWebview } from "../symbols/detail/panel";
import { SymbolTreeProvider } from "../symbols/tree";
import { ViewsProvider } from "../views/tree";
import { renderWorkspaceNode } from "./render";
import { WorkspaceNode, WorkspaceTreeProvider } from "./workspaceTree";
import { RevealCoordinator } from "./revealCoordinator";

export interface WorkspaceFeature {
	tree: WorkspaceTreeProvider;
	syncStats: { revealRequests: number; revealOperations: number };
	onDidSelectSymbol: vscode.Event<SymbolDto>;
	onDidSelectSymbolContext: vscode.Event<{ identity: string; label: string; kind: string }>;
	revealSymbol: (symbol: SymbolDto) => Promise<void>;
}

export interface WorkspaceInputs {
	session: DaemonSession;
	setup: SetupProvider;
	daemons: DaemonListProvider;
	symbols: SymbolTreeProvider;
	views: ViewsProvider;
	changes: ChangesProvider;
	detail: DetailWebview;
	rules: RulesProvider;
	ruleFiles: RuleFilesProvider;
}

export function registerWorkspace(
	context: vscode.ExtensionContext,
	inputs: WorkspaceInputs,
): WorkspaceFeature {
	const provider = new WorkspaceTreeProvider(
		inputs.setup,
		inputs.daemons,
		inputs.symbols,
		inputs.views,
		inputs.changes,
		inputs.rules,
		inputs.ruleFiles,
	);
	const treeView = vscode.window.createTreeView("codeMoniker.workspace", {
		treeDataProvider: provider,
		showCollapseAll: true,
	});
	const symbolSelection = new vscode.EventEmitter<SymbolDto>();
	const contextSelection = new vscode.EventEmitter<{ identity: string; label: string; kind: string }>();
	const syncStats = { revealRequests: 0, revealOperations: 0 };
	const reveals = new RevealCoordinator();

	let pendingSelection: NodeJS.Timeout | undefined;
	const SELECTION_DEBOUNCE_MS = 180;

	context.subscriptions.push(
		provider,
		treeView,
			symbolSelection,
			contextSelection,
			treeView.onDidChangeSelection((event) => {
			const node = event.selection[0];
			if (pendingSelection) {
				clearTimeout(pendingSelection);
				pendingSelection = undefined;
			}
			if (node?.kind === "symbols" && node.node.kind === "symbol") {
				const symbol = node.node.symbol;
				if (reveals.consumeSelection(symbol.uri)) return;
				symbolSelection.fire(symbol);
				pendingSelection = setTimeout(() => {
					pendingSelection = undefined;
					void inputs.detail.showForSymbol(symbol);
				}, SELECTION_DEBOUNCE_MS);
				return;
			}
			reveals.cancel();
			if (node?.kind === "symbols" && node.node.kind === "identity") {
				contextSelection.fire({
					identity: node.node.row.identity,
					label: node.node.label ?? node.node.row.name,
					kind: node.node.row.kind,
				});
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
			reveals.cancel();
		}),
		inputs.session.onWorkspaceEvent((event) => {
			if (event.kind === "stale" || event.kind === "refreshed") {
				inputs.daemons.refresh();
				inputs.symbols.refresh();
				inputs.views.refresh();
				inputs.changes.refresh();
				inputs.rules.refresh();
			}
			if (event.kind === "git_base") {
				inputs.changes.refresh();
			}
		}),
	);

	return {
			tree: provider,
			syncStats,
			onDidSelectSymbol: symbolSelection.event,
			onDidSelectSymbolContext: contextSelection.event,
		revealSymbol: async (symbol: SymbolDto) => {
			syncStats.revealRequests++;
			if (reveals.pending === symbol.uri) return;
			const selected = treeView.selection[0];
			if (
				selected?.kind === "symbols" &&
				selected.node.kind === "symbol" &&
				selected.node.symbol.uri === symbol.uri
			) {
				return;
			}
			const token = reveals.begin(symbol.uri);
			try {
				const path = await provider.findSymbolPath(symbol.uri);
				if (!reveals.isCurrent(token) || !path || path.length === 0) return;
				syncStats.revealOperations++;
				if (!reveals.markProgrammatic(token)) return;
				await treeView.reveal(path[path.length - 1], {
					expand: true,
					select: true,
					focus: false,
				});
			} catch (error) {
				reveals.revealFailed(token);
				throw error;
			} finally {
				reveals.finish(token);
			}
		},
	};
}
