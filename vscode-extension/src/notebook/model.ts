// The on-disk .cmnb document model and the cell metadata carried through VSCode.

export interface CmnbDocument {
	version: number;
	title?: string;
	catalog?: {
		copiedFrom?: string;
	};
	cells: CmnbCell[];
}

export type CmnbCell = MarkdownCell | SampleCell | RuleCell;

export interface MarkdownCell {
	kind: "markdown";
	value: string;
}

export interface SampleCell {
	kind: "sample";
	/** Language id from langs.ts (e.g. "rust"). */
	language: string;
	value: string;
}

export interface RuleCell {
	kind: "rule";
	/** Sample language this rule is evaluated against (id from langs.ts). */
	language: string;
	/** A real .code-moniker.toml fragment: rules, aliases, profiles. */
	value: string;
}

/** Metadata attached to VSCode notebook cells so we can round-trip to .cmnb. */
export interface SampleCellMeta {
	cmType: "sample";
	language: string;
}

export interface RuleCellMeta {
	cmType: "rule";
	language: string;
}
