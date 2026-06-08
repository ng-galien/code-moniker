/** A compiled rule, mirroring `code-moniker rules eval` JSON `rules[]`. */
export interface RuleSpec {
	rule_id: string;
	severity: string;
	domain: string;
	kind?: string | null;
	expr: string;
	expanded_expr: string;
	message?: string | null;
	rationale?: string | null;
	require_doc_comment?: string | null;
}

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

/** Shape of `code-moniker rules eval --format json`. */
export interface EvalReport {
	lang: string;
	rules_file: string;
	total_rules: number;
	total_violations: number;
	rules: RuleSpec[];
	violations: Violation[];
}

/** Shape of `code-moniker check --format json`. */
export interface CheckReport {
	summary: {
		files_scanned: number;
		files_with_violations: number;
		total_violations: number;
		total_errors: number;
		total_warnings: number;
	};
	files: CheckFile[];
	errors: { file: string; error: string }[];
}

export interface CheckFile {
	file: string;
	violations: Violation[];
}

/** Payload emitted to the violations renderer. */
export interface ViolationsPayload {
	language: string;
	sample: string;
	total: number;
	rules: RuleSpec[];
	violations: Violation[];
}

