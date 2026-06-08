import * as path from "node:path";
import * as vscode from "vscode";

import { registerCatalogCommands } from "./commands";
import { CatalogNode } from "./nodes";
import { CatalogNotebookStore } from "./notebooks";
import { CatalogRepository } from "./repository";
import { CatalogProvider } from "./tree";

const CATALOG_MIME = "application/vnd.code-moniker.catalog-entry";
const URI_LIST_MIME = "text/uri-list";

export function registerCatalog(context: vscode.ExtensionContext): void {
	const repository = new CatalogRepository(context);
	const notebooks = new CatalogNotebookStore(context);
	const provider = new CatalogProvider(repository);
	const watcher = vscode.workspace.createFileSystemWatcher("**/*.cmnb");
	context.subscriptions.push(
		watcher,
		watcher.onDidChange(() => provider.refresh()),
		watcher.onDidCreate(() => provider.refresh()),
		watcher.onDidDelete(() => provider.refresh()),
		vscode.workspace.onDidSaveNotebookDocument((notebook) => {
			if (notebook.uri.scheme === "file" && notebook.uri.fsPath.endsWith(".cmnb")) {
				provider.refresh();
			}
		}),
		vscode.window.createTreeView("codeMoniker.catalog", {
			treeDataProvider: provider,
			dragAndDropController: new CatalogDragAndDrop(repository, notebooks, provider),
			showCollapseAll: true,
		}),
	);
	registerCatalogCommands(context, repository, notebooks, provider);
}

class CatalogDragAndDrop implements vscode.TreeDragAndDropController<CatalogNode> {
	readonly dragMimeTypes = [CATALOG_MIME, URI_LIST_MIME];
	readonly dropMimeTypes = [CATALOG_MIME, URI_LIST_MIME];

	constructor(
		private readonly repository: CatalogRepository,
		private readonly notebooks: CatalogNotebookStore,
		private readonly provider: CatalogProvider,
	) {}

	async handleDrag(
		source: readonly CatalogNode[],
		dataTransfer: vscode.DataTransfer,
	): Promise<void> {
		const ids = [...new Set(source.flatMap(builtinEntryId))];
		if (ids.length > 0) {
			dataTransfer.set(CATALOG_MIME, new vscode.DataTransferItem(JSON.stringify(ids)));
		}
		const uris = await this.prepareDraggedUris(ids);
		if (uris.length > 0) {
			dataTransfer.set(URI_LIST_MIME, new vscode.DataTransferItem(uris.join("\r\n")));
		}
	}

	async handleDrop(
		target: CatalogNode | undefined,
		dataTransfer: vscode.DataTransfer,
	): Promise<void> {
		if (!isUserDropTarget(target)) {
			return;
		}
		const item = dataTransfer.get(CATALOG_MIME);
		const copiedInternal = item
			? await copyBuiltinIds(parseIds(await item.asString()), this.repository)
			: 0;
		const copiedFiles = await copyDroppedNotebookFiles(dataTransfer, this.repository);
		const copied = copiedInternal + copiedFiles;
		if (copied > 0) {
			this.provider.refresh();
			void vscode.window.showInformationMessage(`Copied ${copied} catalog sample(s).`);
		}
	}

	private async prepareDraggedUris(ids: string[]): Promise<string[]> {
		const uris: string[] = [];
		for (const id of ids) {
			const entry = await this.repository.findEntry(id);
			if (!entry || entry.source !== "builtin") {
				continue;
			}
			try {
				const uri = this.notebooks.prepareBuiltin(await this.repository.readDocument(entry));
				uris.push(uri.toString());
			} catch {
				// A pack may depend on the CLI; keep drag working for other entries.
			}
		}
		return uris;
	}
}

function builtinEntryId(node: CatalogNode): string[] {
	if (node.kind === "entry" && node.entry.source === "builtin") {
		return [node.entry.id];
	}
	if (node.kind === "rule" && node.item.entry.source === "builtin") {
		return [node.item.entry.id];
	}
	return [];
}

async function copyBuiltinIds(
	ids: string[],
	repository: CatalogRepository,
): Promise<number> {
	let copied = 0;
	for (const id of ids) {
		const entry = await repository.findEntry(id);
		if (!entry || entry.source !== "builtin") {
			continue;
		}
		const result = await repository.copyToUserCatalog(entry);
		if (result.ok) {
			copied++;
		}
	}
	return copied;
}

async function copyDroppedNotebookFiles(
	dataTransfer: vscode.DataTransfer,
	repository: CatalogRepository,
): Promise<number> {
	const item = dataTransfer.get(URI_LIST_MIME);
	if (!item) {
		return 0;
	}
	const folder = await repository.userCatalogFolder();
	if (!folder) {
		void vscode.window.showWarningMessage("Open a workspace folder first.");
		return 0;
	}
	await vscode.workspace.fs.createDirectory(folder);
	let copied = 0;
	for (const uri of parseUriList(await item.asString())) {
		if (uri.scheme !== "file" || !uri.fsPath.endsWith(".cmnb")) {
			continue;
		}
		const target = await uniqueUri(folder, path.basename(uri.fsPath));
		await vscode.workspace.fs.copy(uri, target, { overwrite: false });
		copied++;
	}
	return copied;
}

function isUserDropTarget(target: CatalogNode | undefined): boolean {
	if (!target) {
		return false;
	}
	if (target.kind === "group") {
		return target.groupKind === "user";
	}
	return target.kind === "entry" && target.entry.source === "user";
}

function parseIds(raw: string): string[] {
	try {
		const value = JSON.parse(raw) as unknown;
		return Array.isArray(value)
			? value.filter((item): item is string => typeof item === "string")
			: [];
	} catch {
		return [];
	}
}

function parseUriList(raw: string): vscode.Uri[] {
	return raw
		.split(/\r?\n/)
		.map((line) => line.trim())
		.filter((line) => line && !line.startsWith("#"))
		.map((line) => vscode.Uri.parse(line, true));
}

async function uniqueUri(folder: vscode.Uri, fileName: string): Promise<vscode.Uri> {
	const parsed = path.parse(fileName);
	for (let index = 0; ; index++) {
		const suffix = index === 0 ? "" : ` ${index + 1}`;
		const uri = vscode.Uri.joinPath(folder, `${parsed.name}${suffix}${parsed.ext}`);
		if (!(await exists(uri))) {
			return uri;
		}
	}
}

async function exists(uri: vscode.Uri): Promise<boolean> {
	try {
		await vscode.workspace.fs.stat(uri);
		return true;
	} catch {
		return false;
	}
}
