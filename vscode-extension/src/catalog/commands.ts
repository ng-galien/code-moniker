import * as vscode from "vscode";

import { LANGS, langById } from "../shared/languages";
import { openScenarioDocument, openScenarioFile } from "../scenario/open";
import { CatalogNode } from "./nodes";
import { CatalogNotebookStore, catalogEntryIdFromUri } from "./notebooks";
import { CatalogRepository } from "./repository";
import { CatalogProvider } from "./tree";
import {
	CatalogEntry,
	CatalogFilters,
	CatalogSortMode,
	CatalogSourceFilter,
	CatalogViewMode,
} from "./model";

export function registerCatalogCommands(
	context: vscode.ExtensionContext,
	repository: CatalogRepository,
	notebooks: CatalogNotebookStore,
	provider: CatalogProvider,
): void {
	context.subscriptions.push(
		vscode.commands.registerCommand("codeMoniker.openLesson", (id?: string) =>
			openLesson(id, repository, notebooks),
		),
		vscode.commands.registerCommand("codeMoniker.openConcept", (id?: string) =>
			openConcept(id, repository, notebooks),
		),
		vscode.commands.registerCommand("codeMoniker.openPack", (name?: string) =>
			openPack(name, repository, notebooks),
		),
		vscode.commands.registerCommand("codeMoniker.catalog.openEntry", (target: string | CatalogNode) =>
			openCatalogEntry(entryId(target), repository, notebooks),
		),
		vscode.commands.registerCommand("codeMoniker.catalog.resetEntry", (target: string | CatalogNode) =>
			resetCatalogEntry(entryId(target), repository, notebooks),
		),
		vscode.commands.registerCommand("codeMoniker.catalog.resetActiveEntry", () =>
			resetEntry(activeCatalogEntryId(), repository, notebooks),
		),
		vscode.commands.registerCommand("codeMoniker.catalog.copyToUserCatalog", (target: string | CatalogNode) =>
			copyCatalogEntry(entryId(target), repository, provider),
		),
		vscode.commands.registerCommand("codeMoniker.catalog.refresh", () => provider.refresh()),
		vscode.commands.registerCommand("codeMoniker.catalog.changeView", () =>
			changeView(provider),
		),
		vscode.commands.registerCommand("codeMoniker.catalog.changeSort", () =>
			changeSort(provider),
		),
		vscode.commands.registerCommand("codeMoniker.catalog.filter", () =>
			filterCatalog(provider),
		),
		vscode.commands.registerCommand("codeMoniker.catalog.clearFilters", () =>
			provider.clearFilters(),
		),
		vscode.commands.registerCommand("codeMoniker.catalog.openUserFolder", () =>
			openUserFolder(repository),
		),
	);
}

async function openLesson(
	id: string | undefined,
	repository: CatalogRepository,
	notebooks: CatalogNotebookStore,
): Promise<void> {
	const target =
		id ? `builtin:lesson:${id}` : await pickEntryId(repository, "Open lesson", isBuiltinLesson);
	await openEntry(target, repository, notebooks);
}

async function openConcept(
	id: string | undefined,
	repository: CatalogRepository,
	notebooks: CatalogNotebookStore,
): Promise<void> {
	const target =
		id ? `builtin:concept:${id}` : await pickEntryId(repository, "Open concept", isBuiltinConcept);
	await openEntry(target, repository, notebooks);
}

async function openPack(
	name: string | undefined,
	repository: CatalogRepository,
	notebooks: CatalogNotebookStore,
): Promise<void> {
	const target =
		name ? `builtin:pack:${name}` : await pickEntryId(repository, "Open sample pack", isBuiltinPack);
	await openEntry(target, repository, notebooks);
}

async function openCatalogEntry(
	id: string | undefined,
	repository: CatalogRepository,
	notebooks: CatalogNotebookStore,
): Promise<void> {
	const target = id ?? await pickEntryId(repository, "Open catalog sample");
	await openEntry(target, repository, notebooks);
}

async function openEntry(
	id: string | undefined,
	repository: CatalogRepository,
	notebooks: CatalogNotebookStore,
): Promise<void> {
	if (!id) {
		return;
	}
	const entry = await repository.findEntry(id);
	if (!entry) {
		void vscode.window.showErrorMessage(`Unknown catalog entry "${id}".`);
		return;
	}
	try {
		// Scenarios (multi-file) open with the scenario notebook editor; user
		// scenario files stay file-backed, builtin packs open from their document.
		if (entry.kind === "scenario" && entry.uri) {
			await openScenarioFile(entry.uri);
			return;
		}
		if (entry.kind === "pack" && entry.document !== undefined) {
			await openScenarioDocument(entry.document);
			return;
		}
		if (entry.source === "user" && entry.uri) {
			const notebook = await vscode.workspace.openNotebookDocument(entry.uri);
			await vscode.window.showNotebookDocument(notebook, { preview: false });
			return;
		}
		await notebooks.openBuiltin(await repository.readDocument(entry));
	} catch (err) {
		void vscode.window.showErrorMessage((err as Error).message);
	}
}

async function resetCatalogEntry(
	id: string | undefined,
	repository: CatalogRepository,
	notebooks: CatalogNotebookStore,
): Promise<void> {
	const target =
		id ?? activeCatalogEntryId() ?? await pickEntryId(repository, "Reset catalog sample", isBuiltin);
	await resetEntry(target, repository, notebooks);
}

async function resetEntry(
	id: string | undefined,
	repository: CatalogRepository,
	notebooks: CatalogNotebookStore,
): Promise<void> {
	if (!id) {
		return;
	}
	const entry = await repository.findEntry(id);
	if (!entry || entry.source !== "builtin") {
		void vscode.window.showWarningMessage("Only builtin catalog entries can be reset.");
		return;
	}
	try {
		// Builtin scenarios are not edit-tracked (they open untitled); reset is a
		// fresh open from the pristine document.
		if (entry.kind === "pack" && entry.document !== undefined) {
			await openScenarioDocument(entry.document);
			void vscode.window.showInformationMessage(`Reopened "${entry.title}" from the catalog scenario.`);
			return;
		}
		await notebooks.resetBuiltin(await repository.readDocument(entry));
		void vscode.window.showInformationMessage(`Reset "${entry.title}" to the catalog sample.`);
	} catch (err) {
		void vscode.window.showErrorMessage((err as Error).message);
	}
}

async function copyCatalogEntry(
	id: string | undefined,
	repository: CatalogRepository,
	provider: CatalogProvider,
): Promise<void> {
	const target = id ?? await pickEntryId(repository, "Copy to user catalog", isBuiltin);
	await copyToUserCatalog(target, repository, provider);
}

async function copyToUserCatalog(
	id: string | undefined,
	repository: CatalogRepository,
	provider: CatalogProvider,
): Promise<void> {
	if (!id) {
		return;
	}
	const entry = await repository.findEntry(id);
	if (!entry || entry.source !== "builtin") {
		void vscode.window.showWarningMessage("Only builtin catalog entries can be copied.");
		return;
	}
	try {
		const copied = await repository.copyToUserCatalog(entry);
		if (!copied.ok) {
			void vscode.window.showWarningMessage(copied.error);
			return;
		}
		provider.refresh();
		if (copied.uri.fsPath.endsWith(".md")) {
			await openScenarioFile(copied.uri);
		} else {
			const notebook = await vscode.workspace.openNotebookDocument(copied.uri);
			await vscode.window.showNotebookDocument(notebook, { preview: false });
		}
		void vscode.window.showInformationMessage(
			`Copied "${entry.title}" to ${vscode.workspace.asRelativePath(copied.uri)}.`,
		);
	} catch (err) {
		void vscode.window.showErrorMessage((err as Error).message);
	}
}

async function pickEntryId(
	repository: CatalogRepository,
	title: string,
	filter: (entry: CatalogEntry) => boolean = () => true,
): Promise<string | undefined> {
	const entries = (await repository.entries()).filter(filter);
	if (entries.length === 0) {
		void vscode.window.showWarningMessage("No catalog entry is available.");
		return undefined;
	}
	const pick = await vscode.window.showQuickPick(
		entries.map((entry) => ({
			label: entry.title,
			description: `${entry.source} · ${entry.kind}`,
			detail: entry.blurb,
			entry,
		})),
		{ title },
	);
	return pick?.entry.id;
}

async function changeView(provider: CatalogProvider): Promise<void> {
	const current = provider.getState().viewMode;
	const pick = await vscode.window.showQuickPick(
		[
			{ label: "Learning path", mode: "path" as CatalogViewMode },
			{ label: "By language", mode: "language" as CatalogViewMode },
			{ label: "By rule", mode: "rule" as CatalogViewMode },
		].map((item) => ({
			...item,
			description: item.mode === current ? "current" : undefined,
		})),
		{ title: "Catalog view" },
	);
	if (pick) {
		provider.setViewMode(pick.mode);
	}
}

async function changeSort(provider: CatalogProvider): Promise<void> {
	const current = provider.getState().sortMode;
	const pick = await vscode.window.showQuickPick(
		[
			{ label: "Title", mode: "title" as CatalogSortMode },
			{ label: "Level", mode: "level" as CatalogSortMode },
			{ label: "Source", mode: "source" as CatalogSortMode },
		].map((item) => ({
			...item,
			description: item.mode === current ? "current" : undefined,
		})),
		{ title: "Catalog sort" },
	);
	if (pick) {
		provider.setSortMode(pick.mode);
	}
}

async function filterCatalog(provider: CatalogProvider): Promise<void> {
	const current = provider.getState().filters;
	const source = await vscode.window.showQuickPick(
		[
			{ label: "All sources", source: "all" as CatalogSourceFilter },
			{ label: "Builtin only", source: "builtin" as CatalogSourceFilter },
			{ label: "User only", source: "user" as CatalogSourceFilter },
		].map((item) => ({
			...item,
			description: item.source === current.source ? "current" : undefined,
		})),
		{ title: "Catalog source filter" },
	);
	if (!source) {
		return;
	}
	const language = await vscode.window.showQuickPick(
		[
			{ label: "All languages", language: "all" as const },
			...LANGS.map((lang) => ({ label: lang.label, language: lang.id })),
		].map((item) => ({
			...item,
			description: item.language === current.language ? "current" : undefined,
		})),
		{ title: "Catalog language filter" },
	);
	if (!language) {
		return;
	}
	const query = await vscode.window.showInputBox({
		title: "Catalog search",
		placeHolder: "Optional text in title, tags, description...",
		value: current.query,
	});
	if (query === undefined) {
		return;
	}
	const filters: CatalogFilters = {
		source: source.source,
		language: language.language,
		query,
	};
	provider.setFilters(filters);
}

async function openUserFolder(repository: CatalogRepository): Promise<void> {
	const folder = await repository.userCatalogFolder();
	if (!folder) {
		void vscode.window.showWarningMessage("Open a workspace folder first.");
		return;
	}
	await vscode.workspace.fs.createDirectory(folder);
	await vscode.commands.executeCommand("revealFileInOS", folder);
}

function entryId(target: string | CatalogNode | undefined): string | undefined {
	if (typeof target === "string") {
		return target;
	}
	if (!target) {
		return undefined;
	}
	if (target.kind === "entry") {
		return target.entry.id;
	}
	if (target.kind === "rule") {
		return target.item.entry.id;
	}
	return undefined;
}

function activeCatalogEntryId(): string | undefined {
	const notebook = vscode.window.activeNotebookEditor?.notebook;
	return notebook ? catalogEntryIdFromUri(notebook.uri) : undefined;
}

export function entryLabel(id: string, entries: { id: string; title: string }[]): string {
	return entries.find((entry) => entry.id === id)?.title ?? id;
}

export function languageLabel(language: string): string {
	return language === "all" ? "All languages" : langById(language)?.label ?? language;
}

function isBuiltin(entry: CatalogEntry): boolean {
	return entry.source === "builtin";
}

function isBuiltinLesson(entry: CatalogEntry): boolean {
	return entry.source === "builtin" && entry.kind === "lesson";
}

function isBuiltinConcept(entry: CatalogEntry): boolean {
	return entry.source === "builtin" && entry.kind === "concept";
}

function isBuiltinPack(entry: CatalogEntry): boolean {
	return entry.source === "builtin" && entry.kind === "pack";
}
