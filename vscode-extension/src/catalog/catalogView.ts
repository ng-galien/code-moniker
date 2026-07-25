import * as vscode from "vscode";

import { registerCatalogCommands } from "./commands";
import { CatalogNode } from "./nodes";
import { CatalogRepository } from "./repository";
import { watchAndRefresh } from "../shared/watch";
import { CatalogProvider } from "./tree";

export function registerCatalog(context: vscode.ExtensionContext): void {
	const repository = new CatalogRepository();
	const provider = new CatalogProvider(repository, context.extensionUri);
	const treeView = vscode.window.createTreeView("codeMoniker.catalog", {
		treeDataProvider: provider,
		showCollapseAll: true,
	});
	context.subscriptions.push(
		...watchAndRefresh("**/*.cm.md", () => provider.refresh()),
		treeView,
		vscode.window.onDidChangeActiveNotebookEditor((editor) =>
			revealActiveCatalogEditor(provider, treeView, editor?.notebook.uri),
		),
		vscode.window.onDidChangeActiveTextEditor((editor) =>
			revealActiveCatalogEditor(provider, treeView, editor?.document.uri),
		),
	);
	registerCatalogCommands(context, repository, provider, treeView);
	void revealActiveCatalogEditor(
		provider,
		treeView,
		vscode.window.activeNotebookEditor?.notebook.uri
			?? vscode.window.activeTextEditor?.document.uri,
	);
}

async function revealActiveCatalogEditor(
	provider: CatalogProvider,
	treeView: vscode.TreeView<CatalogNode>,
	uri: vscode.Uri | undefined,
): Promise<void> {
	if (!uri) {
		return;
	}
	const node = await provider.nodeForUri(uri);
	if (!node) {
		return;
	}
	try {
		await treeView.reveal(node, { select: true, focus: false, expand: true });
	} catch {
	}
}
