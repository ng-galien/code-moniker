import { CatalogEntry, CatalogRule } from "./model";

export type CatalogNode =
	| CatalogGroupNode
	| CatalogEntryNode
	| CatalogRuleNode
	| CatalogInfoNode;

export interface CatalogGroupNode {
	kind: "group";
	id: string;
	label: string;
	description?: string;
	groupKind: "builtin" | "learn" | "language" | "rules";
	/** Full learn path for recursive path groups. */
	path?: string;
	/** Parent learn path, absent for top-level groups. */
	parentPath?: string;
	groups?: CatalogGroupNode[];
	entries?: CatalogEntry[];
	rules?: CatalogRule[];
}

export interface CatalogEntryNode {
	kind: "entry";
	entry: CatalogEntry;
}

export interface CatalogRuleNode {
	kind: "rule";
	item: CatalogRule;
}

export interface CatalogInfoNode {
	kind: "info";
	label: string;
	description?: string;
}
