export interface Violation {
	rule_id: string;
	severity: string;
	moniker: string;
	kind: string;
	/** 1-indexed inclusive [start, end]. */
	lines: [number, number];
	/** Technical diagnostic (actual vs expected). */
	message: string;
	/** Authored rule message, rendered with templates. */
	explanation?: string;
}

/** Shape of `code-moniker check --format json`. */
export interface CheckReport {
	summary: {
		files_scanned: number;
		files_with_violations: number;
		total_violations: number;
		total_rule_errors?: number;
		total_errors: number;
		total_warnings: number;
		files_with_errors?: number;
	};
	files: CheckFile[];
	errors?: { file: string; error: string }[];
}

export interface CheckFile {
	file: string;
	violations: Violation[];
}

/** Payload emitted by scenario notebooks for `code-moniker check` results. */
export interface CheckOutputPayload {
	kind: "check";
	target: string;
	summary: CheckReport["summary"];
	files: CheckFile[];
	errors?: { file: string; error: string }[];
}

/** Navigation requests posted from the violations renderer back to the host. */
export type RendererMessage =
	| { command: "revealFile"; file: string }
	| { command: "revealLine"; file: string; line: number }
	| { command: "revealRule"; ruleId: string };
