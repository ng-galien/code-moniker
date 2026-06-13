import * as vscode from "vscode";

import { RuleEntry } from "../rules/parse";

export type CatalogSource = "builtin";
export type CatalogKind = "pack" | "scenario";
export type CatalogLevel = "Learn" | "Practice" | "Reference";
export type CatalogCategory = "learn" | "sample";

export interface CatalogEntry {
	id: string;
	source: CatalogSource;
	kind: CatalogKind;
	category: CatalogCategory;
	title: string;
	fileName: string;
	blurb: string;
	langId?: string;
	level: CatalogLevel;
	tags: string[];
	uri?: vscode.Uri;
	/** Scenario Markdown document for multi-file scenario entries (packs). */
	document?: string;
}

export interface CatalogRule {
	entry: CatalogEntry;
	rule: RuleEntry;
}

export interface CatalogDocument {
	entry: CatalogEntry;
	document: string;
}

export type CatalogViewMode = "path" | "language" | "rule";
export type CatalogSortMode = "title" | "level";

export interface CatalogFilters {
	language: "all" | string;
	query: string;
}

export const DEFAULT_CATALOG_FILTERS: CatalogFilters = {
	language: "all",
	query: "",
};

export const DEFAULT_CATALOG_VIEW_MODE: CatalogViewMode = "path";
export const DEFAULT_CATALOG_SORT_MODE: CatalogSortMode = "title";
