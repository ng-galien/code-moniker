import * as vscode from "vscode";

import { CmnbCell, CmnbDocument } from "../notebook/model";
import { toCellData } from "../notebook/serializer";
import { CatalogDocument, CatalogEntry } from "./model";

const NOTEBOOK_TYPE = "code-moniker";
const CATALOG_SCHEME = "code-moniker-catalog";

interface Session {
	initial: Uint8Array;
	current: Uint8Array;
	ctime: number;
	mtime: number;
}

export class CatalogNotebookStore implements vscode.Disposable {
	private readonly changeEmitter = new vscode.EventEmitter<vscode.FileChangeEvent[]>();
	private readonly sessions = new Map<string, Session>();

	constructor(context: vscode.ExtensionContext) {
		const provider = new CatalogFileSystemProvider(this.sessions, this.changeEmitter);
		context.subscriptions.push(
			vscode.workspace.registerFileSystemProvider(CATALOG_SCHEME, provider, {
				isCaseSensitive: true,
			}),
			vscode.workspace.onDidChangeNotebookDocument((event) => {
				if (event.notebook.uri.scheme === CATALOG_SCHEME) {
					void event.notebook.save();
				}
			}),
			this,
		);
	}

	async openBuiltin(document: CatalogDocument): Promise<void> {
		const uri = this.prepareBuiltin(document);
		const notebook = await vscode.workspace.openNotebookDocument(uri);
		await vscode.window.showNotebookDocument(notebook, { preview: false });
		await notebook.save();
	}

	prepareBuiltin(document: CatalogDocument): vscode.Uri {
		const uri = this.uriFor(document.entry);
		seedSession(this.sessions, uri, document);
		return uri;
	}

	async resetBuiltin(document: CatalogDocument): Promise<void> {
		const uri = this.uriFor(document.entry);
		const bytes = encodeDocument(document);
		const key = uri.toString();
		const existing = this.sessions.get(key);
		this.sessions.set(key, resetSession(existing, bytes));
		this.changeEmitter.fire([{ type: vscode.FileChangeType.Changed, uri }]);

		const open = vscode.workspace.notebookDocuments.find(
			(notebook) => notebook.uri.toString() === key,
		);
		if (open) {
			const edit = new vscode.WorkspaceEdit();
			edit.set(uri, [
				vscode.NotebookEdit.replaceCells(
					new vscode.NotebookRange(0, open.cellCount),
					document.cells.map(toCellData),
				),
			]);
			await vscode.workspace.applyEdit(edit);
			await open.save();
			await vscode.window.showNotebookDocument(open, { preview: false });
		} else {
			await this.openBuiltin(document);
		}
	}

	uriFor(entry: CatalogEntry): vscode.Uri {
		return vscode.Uri.from({
			scheme: CATALOG_SCHEME,
			authority: entry.source,
			path: `/${encodeURIComponent(entry.id)}/${safeFileName(entry.fileName)}`,
		});
	}

	dispose(): void {
		this.changeEmitter.dispose();
	}
}

class CatalogFileSystemProvider implements vscode.FileSystemProvider {
	readonly onDidChangeFile: vscode.Event<vscode.FileChangeEvent[]>;

	constructor(
		private readonly sessions: Map<string, Session>,
		private readonly changeEmitter: vscode.EventEmitter<vscode.FileChangeEvent[]>,
	) {
		this.onDidChangeFile = this.changeEmitter.event;
	}

	watch(): vscode.Disposable {
		return new vscode.Disposable(() => undefined);
	}

	stat(uri: vscode.Uri): vscode.FileStat {
		const session = sessionFor(this.sessions, uri);
		return {
			type: vscode.FileType.File,
			ctime: session.ctime,
			mtime: session.mtime,
			size: session.current.byteLength,
		};
	}

	readDirectory(): [string, vscode.FileType][] {
		return [];
	}

	createDirectory(uri: vscode.Uri): void {
		throw vscode.FileSystemError.NoPermissions(uri);
	}

	readFile(uri: vscode.Uri): Uint8Array {
		return sessionFor(this.sessions, uri).current;
	}

	writeFile(uri: vscode.Uri, content: Uint8Array): void {
		const key = uri.toString();
		this.sessions.set(key, writeSession(this.sessions.get(key), content));
		this.changeEmitter.fire([{ type: vscode.FileChangeType.Changed, uri }]);
	}

	delete(uri: vscode.Uri): void {
		throw vscode.FileSystemError.NoPermissions(uri);
	}

	rename(oldUri: vscode.Uri, newUri: vscode.Uri): void {
		throw vscode.FileSystemError.NoPermissions(oldUri);
	}
}

function seedSession(
	sessions: Map<string, Session>,
	uri: vscode.Uri,
	document: CatalogDocument,
): void {
	const key = uri.toString();
	if (sessions.has(key)) {
		return;
	}
	const bytes = encodeDocument(document);
	const now = Date.now();
	sessions.set(key, { initial: bytes, current: bytes, ctime: now, mtime: now });
}

function resetSession(existing: Session | undefined, content: Uint8Array): Session {
	return {
		initial: existing?.initial ?? content,
		current: content,
		ctime: existing?.ctime ?? Date.now(),
		mtime: Date.now(),
	};
}

function writeSession(existing: Session | undefined, content: Uint8Array): Session {
	return {
		initial: existing?.initial ?? content,
		current: content,
		ctime: existing?.ctime ?? Date.now(),
		mtime: Date.now(),
	};
}

function sessionFor(sessions: Map<string, Session>, uri: vscode.Uri): Session {
	const session = sessions.get(uri.toString());
	if (!session) {
		throw vscode.FileSystemError.FileNotFound(uri);
	}
	return session;
}

function encodeDocument(document: CatalogDocument): Uint8Array {
	const doc: CmnbDocument = {
		version: 1,
		title: document.entry.title,
		catalog: { copiedFrom: document.entry.id },
		cells: document.cells,
	};
	return new TextEncoder().encode(JSON.stringify(doc, null, "\t") + "\n");
}

function safeFileName(fileName: string): string {
	const cleaned = fileName.replace(/[/:\\?%*"<>|]/g, "-").trim();
	return cleaned.endsWith(".cmnb") ? cleaned : `${cleaned}.cmnb`;
}

export function isCatalogNotebook(uri: vscode.Uri): boolean {
	return uri.scheme === CATALOG_SCHEME;
}

export function catalogEntryIdFromUri(uri: vscode.Uri): string | undefined {
	if (!isCatalogNotebook(uri)) {
		return undefined;
	}
	const id = uri.path.split("/").filter(Boolean)[0];
	return id ? decodeURIComponent(id) : undefined;
}

export { NOTEBOOK_TYPE };
