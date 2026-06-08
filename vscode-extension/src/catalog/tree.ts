import * as vscode from "vscode";

import { langById } from "../shared/languages";
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

	constructor(private readonly repository: CatalogRepository) {}

	refresh(): void {
		this.repository.refreshUserEntries();
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

	getTreeItem(node: CatalogNode): vscode.TreeItem {
		if (node.kind === "info") {
			const item = new vscode.TreeItem(node.label);
			item.description = node.description;
			item.iconPath = new vscode.ThemeIcon("info");
			return item;
		}
		if (node.kind === "group") {
			const item = new vscode.TreeItem(
				node.label,
				vscode.TreeItemCollapsibleState.Expanded,
			);
			item.description = node.description;
			item.iconPath = groupIcon(node.groupKind);
			item.contextValue = node.groupKind === "user" ? "cmCatalogUserGroup" : "cmCatalogGroup";
			return item;
		}
		if (node.kind === "entry") {
			return entryTreeItem(node.entry);
		}
		return ruleTreeItem(node.item);
	}
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
	if (filters.source !== "all" && entry.source !== filters.source) {
		return false;
	}
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
	const groups: { id: string; label: string; groupKind: "builtin" | "user" | "kind"; entries: CatalogEntry[] }[] = [
		{
			id: "builtin:lessons",
			label: "Builtin lessons",
			groupKind: "builtin",
			entries: entries.filter((entry) => entry.source === "builtin" && entry.kind === "lesson"),
		},
		{
			id: "builtin:concepts",
			label: "Builtin concepts",
			groupKind: "builtin",
			entries: entries.filter((entry) => entry.source === "builtin" && entry.kind === "concept"),
		},
		{
			id: "builtin:packs",
			label: "Builtin sample packs",
			groupKind: "builtin",
			entries: entries.filter((entry) => entry.source === "builtin" && entry.kind === "pack"),
		},
		{
			id: "user",
			label: "User catalog",
			groupKind: "user",
			entries: entries.filter((entry) => entry.source === "user"),
		},
	];
	return groups
		.filter((group) => group.entries.length > 0 || group.id === "user")
		.map((group) => ({
			kind: "group",
			id: group.id,
			label: group.label,
			description: `${group.entries.length} item(s)`,
			groupKind: group.groupKind,
			entries: group.entries,
		}));
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

function entryTreeItem(entry: CatalogEntry): vscode.TreeItem {
	const item = new vscode.TreeItem(entry.title, vscode.TreeItemCollapsibleState.Collapsed);
	item.description = `${entry.level} · ${catalogLanguageLabel(entry.langId)}`;
	item.tooltip = `${entry.blurb}\n\n${entry.tags.map((tag) => `#${tag}`).join(" ")}`;
	item.iconPath = entryIcon(entry);
	item.contextValue =
		entry.source === "builtin" ? "cmCatalogBuiltinEntry" : "cmCatalogUserEntry";
	item.resourceUri = entry.uri;
	item.command = {
		command: "codeMoniker.catalog.openEntry",
		title: "Open",
		arguments: [entry.id],
	};
	return item;
}

function ruleTreeItem(item: CatalogRule): vscode.TreeItem {
	const rule = item.rule;
	const treeItem = new vscode.TreeItem(rule.id, vscode.TreeItemCollapsibleState.None);
	treeItem.description = `${rule.scope} · ${item.entry.title}`;
	treeItem.tooltip = rule.blockText;
	treeItem.iconPath = new vscode.ThemeIcon(rule.severity === "warn" ? "warning" : "shield");
	treeItem.contextValue =
		item.entry.source === "builtin" ? "cmCatalogBuiltinRule" : "cmCatalogUserRule";
	treeItem.command = {
		command: "codeMoniker.catalog.openEntry",
		title: "Open sample",
		arguments: [item.entry.id],
	};
	return treeItem;
}

function groupIcon(kind: "builtin" | "user" | "language" | "rules" | "kind"): vscode.ThemeIcon {
	if (kind === "user") {
		return new vscode.ThemeIcon("account");
	}
	if (kind === "language") {
		return new vscode.ThemeIcon("symbol-keyword");
	}
	if (kind === "rules") {
		return new vscode.ThemeIcon("law");
	}
	return new vscode.ThemeIcon("library");
}

function entryIcon(entry: CatalogEntry): vscode.ThemeIcon {
	if (entry.source === "user") {
		return new vscode.ThemeIcon("notebook");
	}
	if (entry.kind === "pack") {
		return new vscode.ThemeIcon("package");
	}
	if (entry.kind === "concept") {
		return new vscode.ThemeIcon("book");
	}
	return new vscode.ThemeIcon("notebook");
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
	if (sortMode === "source") {
		const bySource = left.source.localeCompare(right.source);
		if (bySource !== 0) {
			return bySource;
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
		filters.source === "all" ? undefined : filters.source,
		filters.language === "all" ? undefined : filters.language,
		filters.query.trim() ? `"${filters.query.trim()}"` : undefined,
	].filter((part): part is string => Boolean(part));
	return parts.join(" · ");
}
