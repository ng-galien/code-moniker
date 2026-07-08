import * as vscode from "vscode";

import { langById } from "../shared/languages";
import { catalogEntryIcon, catalogGroupIcon, infoIcon, ruleIcon } from "../shared/appIcons";
import {
	CatalogEntry,
	CatalogFilters,
	CatalogRule,
	CatalogSortMode,
	CatalogViewMode,
	DEFAULT_CATALOG_FILTERS,
	DEFAULT_CATALOG_SORT_MODE,
	DEFAULT_CATALOG_VIEW_MODE,
} from "./model";
import { CatalogNode } from "./nodes";
import {
	CatalogRepository,
	catalogLanguageIds,
	catalogLanguageLabel,
} from "./repository";

export class CatalogProvider implements vscode.TreeDataProvider<CatalogNode> {
	private readonly emitter = new vscode.EventEmitter<CatalogNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;

	private viewMode: CatalogViewMode = DEFAULT_CATALOG_VIEW_MODE;
	private filters: CatalogFilters = { ...DEFAULT_CATALOG_FILTERS };
	private sortMode: CatalogSortMode = DEFAULT_CATALOG_SORT_MODE;

	constructor(
		private readonly repository: CatalogRepository,
		private readonly extensionUri: vscode.Uri,
	) {}

	refresh(): void {
		this.repository.refresh();
		this.emitter.fire(undefined);
	}

	setViewMode(mode: CatalogViewMode): void {
		this.viewMode = mode;
		this.emitter.fire(undefined);
	}

	setSortMode(mode: CatalogSortMode): void {
		this.sortMode = mode;
		this.emitter.fire(undefined);
	}

	setFilters(filters: CatalogFilters): void {
		this.filters = filters;
		this.emitter.fire(undefined);
	}

	clearFilters(): void {
		this.filters = { ...DEFAULT_CATALOG_FILTERS };
		this.emitter.fire(undefined);
	}

	getState(): {
		viewMode: CatalogViewMode;
		sortMode: CatalogSortMode;
		filters: CatalogFilters;
	} {
		return {
			viewMode: this.viewMode,
			sortMode: this.sortMode,
			filters: { ...this.filters },
		};
	}

	async getChildren(node?: CatalogNode): Promise<CatalogNode[]> {
		if (!node) {
			const entries = await filteredEntries(
				this.repository,
				this.filters,
				this.sortMode,
			);
			return rootNodes(this.repository, this.viewMode, this.filters, entries);
		}
		if (node.kind === "group") {
			if (node.rules) {
				return node.rules.map((item) => ({ kind: "rule", item }));
			}
			return (node.entries ?? []).map((entry) => ({ kind: "entry", entry }));
		}
		if (node.kind === "entry") {
			const rules = await this.repository.rulesFor(node.entry);
			return rules.length
				? rules.map((item) => ({ kind: "rule", item }))
				: [{ kind: "info", label: "No rules in this sample" }];
		}
		return [];
	}

	async getParent(node: CatalogNode): Promise<CatalogNode | undefined> {
		if (node.kind === "entry") {
			return this.parentForEntry(node.entry);
		}
		if (node.kind === "rule") {
			return this.parentForRule(node.item);
		}
		return undefined;
	}

	getTreeItem(node: CatalogNode): vscode.TreeItem {
		if (node.kind === "info") {
			const item = new vscode.TreeItem(node.label);
			item.description = node.description;
			item.iconPath = infoIcon();
			return item;
		}
		if (node.kind === "group") {
			const item = new vscode.TreeItem(
				node.label,
				vscode.TreeItemCollapsibleState.Expanded,
			);
			item.id = node.id;
			item.description = node.description;
			item.tooltip = groupTooltip(node.groupKind);
			item.iconPath = groupIcon(node.groupKind);
			item.contextValue = "cmCatalogGroup";
			return item;
		}
		if (node.kind === "entry") {
			return entryTreeItem(node.entry, this.extensionUri);
		}
		return ruleTreeItem(node.item);
	}

	async nodeForUri(uri: vscode.Uri): Promise<CatalogNode | undefined> {
		const entry = await entryForUri(this.repository, uri);
		return entry ? { kind: "entry", entry } : undefined;
	}

	private parentForEntry(entry: CatalogEntry): CatalogNode {
		if (this.viewMode === "language") {
			return {
				kind: "group",
				id: `language:${entry.langId ?? "multi"}`,
				label: entry.langId ? langById(entry.langId)?.label ?? entry.langId : "Multi-language",
				groupKind: "language",
			};
		}
		if (entry.category === "learn") {
			return {
				kind: "group",
				id: "builtin:learn",
				label: "Learn syntax",
				groupKind: "learn",
			};
		}
		return {
			kind: "group",
			id: "builtin:packs",
			label: "Operational samples",
			groupKind: "builtin",
		};
	}

	private parentForRule(item: CatalogRule): CatalogNode {
		if (this.viewMode === "rule") {
			return {
				kind: "group",
				id: `rule:${item.rule.id}`,
				label: item.rule.id,
				groupKind: "rules",
			};
		}
		return { kind: "entry", entry: item.entry };
	}
}

async function entryForUri(
	repository: CatalogRepository,
	uri: vscode.Uri,
): Promise<CatalogEntry | undefined> {
	const entries = await repository.entries();
	return entries.find((entry) => entryMatchesUri(entry, uri));
}

function entryMatchesUri(entry: CatalogEntry, uri: vscode.Uri): boolean {
	if (entry.uri?.toString() === uri.toString()) {
		return true;
	}
	return false;
}

async function rootNodes(
	repository: CatalogRepository,
	viewMode: CatalogViewMode,
	filters: CatalogFilters,
	entries: CatalogEntry[],
): Promise<CatalogNode[]> {
	if (entries.length === 0) {
		return [
			{
				kind: "info",
				label: "No catalog entries match the current filters",
				description: filterDescription(filters),
			},
		];
	}
	if (viewMode === "language") {
		return groupByLanguage(entries);
	}
	if (viewMode === "rule") {
		return groupByRule(repository, entries);
	}
	return groupByPath(entries);
}

async function groupByRule(
	repository: CatalogRepository,
	entries: CatalogEntry[],
): Promise<CatalogNode[]> {
	const byRule = new Map<string, CatalogRule[]>();
	for (const entry of entries) {
		let rules: CatalogRule[] = [];
		try {
			rules = await repository.rulesFor(entry);
		} catch {
			rules = [];
		}
		for (const item of rules) {
			const bucket = byRule.get(item.rule.id) ?? [];
			bucket.push(item);
			byRule.set(item.rule.id, bucket);
		}
	}
	return [...byRule.entries()]
		.sort(([left], [right]) => left.localeCompare(right))
		.map(([ruleId, rules]) => ({
			kind: "group",
			id: `rule:${ruleId}`,
			label: ruleId,
			description: `${rules.length} sample(s)`,
			groupKind: "rules",
			rules,
		}));
}

async function filteredEntries(
	repository: CatalogRepository,
	filters: CatalogFilters,
	sortMode: CatalogSortMode,
): Promise<CatalogEntry[]> {
	const entries = await repository.entries();
	const query = filters.query.trim().toLowerCase();
	const filtered = entries.filter((entry) => entryMatches(entry, filters, query));
	return filtered.sort((left, right) => compareEntries(left, right, sortMode));
}

function entryMatches(
	entry: CatalogEntry,
	filters: CatalogFilters,
	query: string,
): boolean {
	if (
		filters.language !== "all" &&
		entry.langId !== filters.language &&
		!entry.tags.includes(filters.language)
	) {
		return false;
	}
	if (!query) {
		return true;
	}
	const haystack = [
		entry.title,
		entry.blurb,
		entry.kind,
		entry.level,
		catalogLanguageLabel(entry.langId),
		...entry.tags,
	]
		.join(" ")
		.toLowerCase();
	return haystack.includes(query);
}

function groupByPath(entries: CatalogEntry[]): CatalogNode[] {
	const learn = entries.filter((entry) => entry.category === "learn");
	const samples = entries.filter((entry) => entry.category === "sample");
	const nodes: CatalogNode[] = [];
	if (learn.length > 0) {
		nodes.push({
			kind: "group",
			id: "builtin:learn",
			label: "Learn syntax",
			description: `${learn.length} scenario(s)`,
			groupKind: "learn",
			entries: learn,
		});
	}
	if (samples.length > 0) {
		nodes.push({
			kind: "group",
			id: "builtin:packs",
			label: "Operational samples",
			description: `${samples.length} scenario(s)`,
			groupKind: "builtin",
			entries: samples,
		});
	}
	return nodes;
}

function groupByLanguage(entries: CatalogEntry[]): CatalogNode[] {
	const languageIds = catalogLanguageIds(entries);
	const nodes = languageIds.map((langId): CatalogNode => {
		const grouped = entries.filter((entry) => entry.langId === langId);
		return {
			kind: "group",
			id: `language:${langId}`,
			label: langById(langId)?.label ?? langId,
			description: `${grouped.length} item(s)`,
			groupKind: "language",
			entries: grouped,
		};
	});
	const multi = entries.filter((entry) => !entry.langId);
	if (multi.length > 0) {
		nodes.push({
			kind: "group",
			id: "language:multi",
			label: "Multi-language",
			description: `${multi.length} item(s)`,
			groupKind: "language",
			entries: multi,
		});
	}
	return nodes;
}

function entryTreeItem(entry: CatalogEntry, extensionUri: vscode.Uri): vscode.TreeItem {
	const item = new vscode.TreeItem(entry.title, vscode.TreeItemCollapsibleState.Collapsed);
	item.id = entry.id;
	item.description = `${entry.level} · ${catalogLanguageLabel(entry.langId)}`;
	item.tooltip = entryTooltip(entry);
	item.iconPath = entryIcon(entry, extensionUri);
	item.contextValue = "cmCatalogBuiltinEntry";
	item.resourceUri = entry.uri;
	item.command = {
		command: "codeMoniker.catalog.openEntry",
		title: "Open Catalog Sample",
		arguments: [{ id: entry.id }],
	};
	return item;
}

function ruleTreeItem(item: CatalogRule): vscode.TreeItem {
	const rule = item.rule;
	const treeItem = new vscode.TreeItem(rule.id, vscode.TreeItemCollapsibleState.None);
	treeItem.id = `${item.entry.id}:rule:${rule.id}`;
	treeItem.description = `${rule.scope} · ${item.entry.title}`;
	treeItem.tooltip = ruleTooltip(item);
	treeItem.iconPath = ruleIcon(rule.severity);
	treeItem.contextValue = "cmCatalogBuiltinRule";
	treeItem.command = {
		command: "codeMoniker.catalog.openEntry",
		title: "Open Catalog Sample",
		arguments: [{ id: item.entry.id }],
	};
	return treeItem;
}

function groupTooltip(kind: "builtin" | "learn" | "language" | "rules"): string {
	if (kind === "builtin") {
		return "Operational scenario samples open as editable clean clones. Save only when you want to keep changes.";
	}
	if (kind === "learn") {
		return "Executable learning scenarios for understanding the rule syntax by running short examples.";
	}
	if (kind === "rules") {
		return "Rules found in executable scenarios. Open a rule to inspect the scenario that demonstrates it.";
	}
	if (kind === "language") {
		return "Executable scenarios grouped by language.";
	}
	return "Executable scenarios.";
}

function entryTooltip(entry: CatalogEntry): string {
	return [
		entry.blurb,
		"",
		"Opens as an editable clean scenario clone. Save it only if you want to keep changes.",
		"",
		entry.tags.map((tag) => `#${tag}`).join(" "),
	].join("\n");
}

function ruleTooltip(item: CatalogRule): string {
	return `This rule belongs to an editable executable scenario.\n\n${item.rule.blockText}`;
}

function groupIcon(kind: "builtin" | "learn" | "language" | "rules"): vscode.ThemeIcon {
	return catalogGroupIcon(kind);
}

function entryIcon(entry: CatalogEntry, extensionUri: vscode.Uri): vscode.ThemeIcon {
	void entry;
	void extensionUri;
	return catalogEntryIcon();
}

function compareEntries(
	left: CatalogEntry,
	right: CatalogEntry,
	sortMode: CatalogSortMode,
): number {
	if (sortMode === "level") {
		const byLevel = levelRank(left.level) - levelRank(right.level);
		if (byLevel !== 0) {
			return byLevel;
		}
	}
	return left.title.localeCompare(right.title);
}

function levelRank(level: string): number {
	if (level === "Basics") {
		return 0;
	}
	if (level === "Practice") {
		return 1;
	}
	return 2;
}

function filterDescription(filters: CatalogFilters): string {
	const parts = [
		filters.language === "all" ? undefined : filters.language,
		filters.query.trim() ? `"${filters.query.trim()}"` : undefined,
	].filter((part): part is string => Boolean(part));
	return parts.join(" · ");
}
