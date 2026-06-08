import * as vscode from "vscode";

import { CmnbCell } from "../notebook/model";
import { RuleEntry } from "../rules/parse";

export type CatalogSource = "builtin" | "user";
export type CatalogKind = "lesson" | "concept" | "pack" | "notebook";
export type CatalogLevel = "Basics" | "Practice" | "Reference";

export interface CatalogEntry {
	id: string;
	source: CatalogSource;
	kind: CatalogKind;
	title: string;
	fileName: string;
	blurb: string;
	langId?: string;
	level: CatalogLevel;
	tags: string[];
	uri?: vscode.Uri;
}

export interface CatalogRule {
	entry: CatalogEntry;
	rule: RuleEntry;
}

export interface CatalogDocument {
	entry: CatalogEntry;
	cells: CmnbCell[];
}

export type CatalogViewMode = "path" | "language" | "rule";
export type CatalogSourceFilter = "all" | CatalogSource;
export type CatalogSortMode = "title" | "level" | "source";

export interface CatalogFilters {
	source: CatalogSourceFilter;
	language: "all" | string;
	query: string;
}

export const DEFAULT_CATALOG_FILTERS: CatalogFilters = {
	source: "all",
	language: "all",
	query: "",
};

export const DEFAULT_CATALOG_VIEW_MODE: CatalogViewMode = "path";
export const DEFAULT_CATALOG_SORT_MODE: CatalogSortMode = "title";
