import * as vscode from "vscode";

import { LANGS, langById } from "../shared/languages";
import { openScenarioDocument, openScenarioFile } from "../scenario/open";
import { CatalogNode } from "./nodes";
import { CatalogRepository } from "./repository";
import { CatalogProvider } from "./tree";
import {
	CatalogEntry,
	CatalogFilters,
	CatalogSortMode,
	CatalogViewMode,
} from "./model";

let catalogOutput: vscode.OutputChannel | undefined;

export function registerCatalogCommands(
	context: vscode.ExtensionContext,
	repository: CatalogRepository,
	provider: CatalogProvider,
	treeView: vscode.TreeView<CatalogNode>,
): void {
	const output = vscode.window.createOutputChannel("Code Moniker Catalog");
	catalogOutput = output;
	context.subscriptions.push(
		output,
		registerCatalogCommand("codeMoniker.openPack", "Open sample pack", (name?: unknown) =>
			openPack(typeof name === "string" ? name : undefined, repository, context.globalStorageUri),
		),
		registerCatalogCommand("codeMoniker.catalog.openEntry", "Open catalog sample", (...targets: unknown[]) =>
			openCatalogEntry(targets, repository, treeView, context.globalStorageUri),
		),
		registerCatalogCommand("codeMoniker.catalog.refresh", "Refresh catalog", () => provider.refresh()),
		registerCatalogCommand("codeMoniker.catalog.changeView", "Change catalog view", () =>
			changeView(provider),
		),
		registerCatalogCommand("codeMoniker.catalog.changeSort", "Sort catalog", () =>
			changeSort(provider),
		),
		registerCatalogCommand("codeMoniker.catalog.filter", "Filter catalog", () =>
			filterCatalog(provider),
		),
		registerCatalogCommand("codeMoniker.catalog.clearFilters", "Clear catalog filters", () =>
			provider.clearFilters(),
		),
	);
}

function registerCatalogCommand(
	command: string,
	label: string,
	callback: (...args: unknown[]) => Promise<unknown> | unknown,
): vscode.Disposable {
	return vscode.commands.registerCommand(command, async (...args: unknown[]) => {
		try {
			appendCatalogLog(`${label} started.`);
			await callback(...args);
		} catch (err) {
			showCatalogError(label, err);
			throw err;
		}
	});
}

async function openPack(
	name: string | undefined,
	repository: CatalogRepository,
	storageUri: vscode.Uri,
): Promise<void> {
	const target =
		name ? `builtin:pack:${name}` : await pickEntryId(repository, "Open sample pack", isBuiltinPack);
	await openEntry(target, repository, storageUri);
}

async function openCatalogEntry(
	targets: unknown[],
	repository: CatalogRepository,
	treeView: vscode.TreeView<CatalogNode>,
	storageUri: vscode.Uri,
): Promise<void> {
	const target = entryIdFromTargets(targets, treeView, targets.length === 0);
	if (target) {
		await openEntry(target, repository, storageUri);
		return;
	}
	if (targets.length > 0) {
		showCatalogWarning("Select a catalog sample, not a catalog group.");
		return;
	}
	const picked = await pickEntryId(repository, "Open catalog sample");
	await openEntry(picked, repository, storageUri);
}

async function openEntry(
	id: string | undefined,
	repository: CatalogRepository,
	storageUri: vscode.Uri,
): Promise<void> {
	if (!id) {
		return;
	}
	const entry = await repository.findEntry(id);
	if (!entry) {
		showCatalogWarning(`Unknown catalog entry "${id}".`);
		return;
	}
	if (entry.kind === "scenario" && entry.uri) {
		await openScenarioFile(entry.uri);
		return;
	}
	if (entry.document === undefined) {
		showCatalogWarning("This catalog entry has no scenario document.");
		return;
	}
	await openScenarioDocument(entry.document, {
		id: entry.id,
		fileName: entry.fileName,
		storageUri,
	});
}

async function pickEntryId(
	repository: CatalogRepository,
	title: string,
	filter: (entry: CatalogEntry) => boolean = () => true,
): Promise<string | undefined> {
	const entries = (await repository.entries()).filter(filter);
	if (entries.length === 0) {
		showCatalogWarning("No catalog entry is available.");
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
		language: language.language,
		query,
	};
	provider.setFilters(filters);
}

function entryId(
	target: unknown,
	treeView?: vscode.TreeView<CatalogNode>,
): string | undefined {
	if (typeof target === "string") {
		return target;
	}
	if (!target) {
		return selectedEntryId(treeView);
	}
	if (isCatalogEntryNode(target)) {
		return target.entry.id;
	}
	if (isCatalogRuleNode(target)) {
		return target.item.entry.id;
	}
	const id = (target as { id?: unknown }).id;
	if (isCatalogEntryId(id)) {
		return id;
	}
	if (typeof id === "string") {
		const ruleIndex = id.indexOf(":rule:");
		if (ruleIndex > 0) {
			const entryId = id.slice(0, ruleIndex);
			if (isCatalogEntryId(entryId)) {
				return entryId;
			}
		}
	}
	const commandArgs = (target as { command?: { arguments?: unknown[] } }).command?.arguments;
	if (commandArgs) {
		return entryIdFromTargets(commandArgs, treeView, false);
	}
	return undefined;
}

function entryIdFromTargets(
	targets: unknown[],
	treeView?: vscode.TreeView<CatalogNode>,
	includeSelection = true,
): string | undefined {
	for (const target of targets) {
		const id = entryId(target, treeView);
		if (id) {
			return id;
		}
		if (Array.isArray(target)) {
			const nested = entryIdFromTargets(target, treeView, false);
			if (nested) {
				return nested;
			}
		}
	}
	return includeSelection ? selectedEntryId(treeView) : undefined;
}

function isCatalogEntryNode(target: unknown): target is Extract<CatalogNode, { kind: "entry" }> {
	return typeof target === "object"
		&& target !== null
		&& (target as { kind?: unknown }).kind === "entry"
		&& typeof (target as { entry?: { id?: unknown } }).entry?.id === "string";
}

function isCatalogRuleNode(target: unknown): target is Extract<CatalogNode, { kind: "rule" }> {
	return typeof target === "object"
		&& target !== null
		&& (target as { kind?: unknown }).kind === "rule"
		&& typeof (target as { item?: { entry?: { id?: unknown } } }).item?.entry?.id === "string";
}

function selectedEntryId(treeView: vscode.TreeView<CatalogNode> | undefined): string | undefined {
	const selected = treeView?.selection[0];
	if (!selected) {
		return undefined;
	}
	return entryId(selected);
}

function isCatalogEntryId(id: unknown): id is string {
	return typeof id === "string" && id.startsWith("builtin:pack:");
}

export function entryLabel(id: string, entries: { id: string; title: string }[]): string {
	return entries.find((entry) => entry.id === id)?.title ?? id;
}

export function languageLabel(language: string): string {
	return language === "all" ? "All languages" : langById(language)?.label ?? language;
}

function isBuiltinPack(entry: CatalogEntry): boolean {
	return entry.source === "builtin" && entry.kind === "pack";
}

function showCatalogWarning(message: string): void {
	appendCatalogLog(`Warning: ${message}`);
	void vscode.window.showWarningMessage(message);
}

function showCatalogError(label: string, err: unknown): void {
	const message = err instanceof Error ? err.message : String(err);
	appendCatalogLog(`${label} failed: ${message}`);
	if (err instanceof Error && err.stack) {
		appendCatalogLog(err.stack);
	}
	void vscode.window.showErrorMessage(
		`Code Moniker: ${label} failed: ${message}`,
		"Show Log",
	).then((action) => {
		if (action === "Show Log") {
			catalogOutput?.show(true);
		}
	});
}

function appendCatalogLog(message: string): void {
	catalogOutput?.appendLine(`[${new Date().toISOString()}] ${message}`);
}
