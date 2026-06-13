import * as vscode from "vscode";

import { registerCatalogCommands } from "./commands";
import { CatalogNode } from "./nodes";
import { CatalogRepository } from "./repository";
import { CatalogProvider } from "./tree";

export function registerCatalog(context: vscode.ExtensionContext): void {
	const repository = new CatalogRepository();
	const provider = new CatalogProvider(repository, context.extensionUri);
	const watcher = vscode.workspace.createFileSystemWatcher("**/*.cm.md");
	const treeView = vscode.window.createTreeView("codeMoniker.catalog", {
		treeDataProvider: provider,
		showCollapseAll: true,
	});
	context.subscriptions.push(
		watcher,
		watcher.onDidChange(() => provider.refresh()),
		watcher.onDidCreate(() => provider.refresh()),
		watcher.onDidDelete(() => provider.refresh()),
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
		// Reveal is best-effort; it can fail while the tree is rebuilding.
	}
}
