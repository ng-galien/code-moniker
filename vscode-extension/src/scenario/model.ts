// Scenario notebooks: a Markdown document (docs/check-scenarios.md in the main
// repo) opened as an executable notebook. Cells mirror the document blocks.

export const SCENARIO_NOTEBOOK_TYPE = "code-moniker-scenario";

export type ScenarioCell =
	| { kind: "markup"; value: string }
	| { kind: "rules"; value: string }
	| { kind: "file"; path: string; fence: string; value: string }
	| { kind: "expect"; value: string };

export interface ScenarioDocument {
	/** Raw front matter lines, without the `---` delimiters. */
	frontMatter?: string;
	cells: ScenarioCell[];
}

/** Metadata attached to VSCode notebook cells for round-tripping. */
export interface ScenarioCellMeta {
	cmType: "rules" | "file" | "expect";
	/** Workspace-relative path for `cm:file` cells. */
	path?: string;
	/** Original fence language tag for `cm:file` cells. */
	fence?: string;
}

export interface ScenarioNotebookMeta {
	frontMatter?: string;
}
