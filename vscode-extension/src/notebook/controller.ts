import * as vscode from "vscode";

import { runEval, validateRulesToml } from "../cli/facade";
import {
	RuleSpec,
	Violation,
	ViolationsPayload,
} from "../cli/model";
import { toDiagnostic } from "../diagnostics/vscode";
import { LANGS, LangDef, langById } from "../shared/languages";
import {
	RuleCellMeta,
	SampleCellMeta,
} from "./model";
import { followingRuleCells, sampleContext } from "./cells";
import { rootOf } from "../shared/workspace";

const VIOLATIONS_MIME = "application/x-code-moniker-violations+json";
const RULE_LANGUAGE_ID = "cmrule-toml";

// Kernel for .cmnb notebooks: running a sample cell checks the following rule
// cells against that code; running a rule cell validates the TOML fragment.
export class CmnbController {
	private readonly controller: vscode.NotebookController;
	private readonly diagnostics: vscode.DiagnosticCollection;

	constructor(private readonly fallbackRoot: string) {
		this.diagnostics = vscode.languages.createDiagnosticCollection(
			"code-moniker-notebook",
		);
		this.controller = vscode.notebooks.createNotebookController(
			"code-moniker-rules",
			"code-moniker",
			"Code Moniker Rules",
		);
		this.controller.supportedLanguages = [
			RULE_LANGUAGE_ID,
			...LANGS.map((lang) => lang.vscodeId),
		];
		this.controller.supportsExecutionOrder = true;
		this.controller.description =
			"Run sample cells against following rules; validate rule cells.";
		this.controller.executeHandler = this.execute.bind(this);
	}

	dispose(): void {
		this.diagnostics.dispose();
		this.controller.dispose();
	}

	private async execute(
		cells: vscode.NotebookCell[],
		notebook: vscode.NotebookDocument,
	): Promise<void> {
		for (const cell of cells) {
			await this.runCell(cell, notebook);
		}
	}

	private async runCell(
		cell: vscode.NotebookCell,
		notebook: vscode.NotebookDocument,
	): Promise<void> {
		const meta = cell.metadata as Partial<RuleCellMeta | SampleCellMeta> | undefined;
		if (meta?.cmType === "sample") {
			await this.runSampleCell(cell, notebook, meta);
		} else if (meta?.cmType === "rule") {
			await this.runRuleCell(cell, notebook);
		}
	}

	private async runRuleCell(
		cell: vscode.NotebookCell,
		notebook: vscode.NotebookDocument,
	): Promise<void> {
		const execution = this.startExecution(cell);
		this.diagnostics.delete(cell.document.uri);

		const rulesToml = cell.document.getText().trim();
		if (!rulesToml) {
			await this.failCell(execution, cell, "The rule cell is empty.");
			return;
		}

		const result = await validateRulesToml(
			rootOf(notebook.uri, this.fallbackRoot),
			rulesToml,
		);
		if (!result.ok) {
			await this.failCell(execution, cell, result.error);
			return;
		}

		await execution.replaceOutput(textOutput("✓ Rule fragment compiles."));
		execution.end(true, Date.now());
	}

	private async runSampleCell(
		cell: vscode.NotebookCell,
		notebook: vscode.NotebookDocument,
		meta: Partial<SampleCellMeta>,
	): Promise<void> {
		const execution = this.startExecution(cell);
		this.diagnostics.delete(cell.document.uri);

		const request = resolveSampleCheck(cell, meta);
		if (!request.ok) {
			await execution.replaceOutput(errorOutput(request.error));
			execution.end(false, Date.now());
			return;
		}

		const result = await evaluateRulesForSample(request, this.diagnostics);
		if (!result.ok) {
			await execution.replaceOutput(errorOutput(result.error));
			execution.end(false, Date.now());
			return;
		}

		const payload = violationsPayload(request, result);
		this.diagnostics.set(cell.document.uri, payload.violations.map(toDiagnostic));
		await execution.replaceOutput(
			new vscode.NotebookCellOutput([
				vscode.NotebookCellOutputItem.json(payload, VIOLATIONS_MIME),
				vscode.NotebookCellOutputItem.text(textSummary(payload), "text/plain"),
			]),
		);
		execution.end(payload.total === 0, Date.now());
	}

	private startExecution(
		cell: vscode.NotebookCell,
	): vscode.NotebookCellExecution {
		const execution = this.controller.createNotebookCellExecution(cell);
		execution.start(Date.now());
		execution.clearOutput();
		return execution;
	}

	private async failCell(
		execution: vscode.NotebookCellExecution,
		cell: vscode.NotebookCell,
		message: string,
	): Promise<void> {
		this.diagnostics.set(cell.document.uri, [cellDiagnostic(message)]);
		await execution.replaceOutput(errorOutput(message));
		execution.end(false, Date.now());
	}
}

interface SampleCheckRequest {
	lang: LangDef;
	language: string;
	sample: string;
	rules: RuleCellRequest[];
}

interface RuleCellRequest {
	cell: vscode.NotebookCell;
	rulesToml: string;
}

type SampleCheckResolution =
	| ({ ok: true } & SampleCheckRequest)
	| { ok: false; error: string };

interface SampleEvaluationResult {
	rules: RuleSpec[];
	violations: Violation[];
}

type SampleEvaluation =
	| ({ ok: true } & SampleEvaluationResult)
	| { ok: false; error: string };

function resolveSampleCheck(
	cell: vscode.NotebookCell,
	meta: Partial<SampleCellMeta>,
): SampleCheckResolution {
	const language = meta.language ?? "";
	const lang = langById(language);
	if (!lang) {
		return { ok: false, error: `Unknown sample language \`${meta.language}\`.` };
	}
	const sample = sampleContext(cell);
	const rules = sample ? followingRuleCells(sample) : [];
	if (rules.length === 0) {
		return {
			ok: false,
			error:
				`No ${lang.label} rule cell found below this sample. ` +
				"Add a rule cell after the code sample, then run the sample cell.",
		};
	}
	return {
		ok: true,
		lang,
		language,
		sample: cell.document.getText(),
		rules,
	};
}

async function evaluateRulesForSample(
	request: SampleCheckRequest,
	diagnostics: vscode.DiagnosticCollection,
): Promise<SampleEvaluation> {
	const rules: RuleSpec[] = [];
	const violations: Violation[] = [];
	for (const rule of request.rules) {
		diagnostics.delete(rule.cell.document.uri);
		const result = await runEval({
			rulesToml: rule.rulesToml,
			cliTag: request.lang.cliTag,
			source: request.sample,
		});
		if (!result.ok) {
			diagnostics.set(rule.cell.document.uri, [cellDiagnostic(result.error)]);
			return { ok: false, error: result.error };
		}
		rules.push(...result.report.rules);
		violations.push(...result.report.violations);
	}
	return { ok: true, rules, violations };
}

function violationsPayload(
	request: SampleCheckRequest,
	result: SampleEvaluationResult,
): ViolationsPayload {
	return {
		language: request.language || request.lang.id,
		sample: request.sample,
		total: result.violations.length,
		rules: result.rules,
		violations: result.violations,
	};
}

function textSummary(payload: ViolationsPayload): string {
	const head =
		payload.total === 0
			? `✓ ${payload.rules.length} rule(s), no violations`
			: `✗ ${payload.rules.length} rule(s), ${payload.total} violation(s)`;
	const lines = payload.violations.map(
		(v) => `  L${v.lines[0]}-L${v.lines[1]} [${v.rule_id}] ${v.explanation ?? v.message}`,
	);
	return [head, ...lines].join("\n");
}

function cellDiagnostic(message: string): vscode.Diagnostic {
	return new vscode.Diagnostic(
		new vscode.Range(0, 0, 0, 4096),
		message,
		vscode.DiagnosticSeverity.Error,
	);
}

function textOutput(message: string): vscode.NotebookCellOutput {
	return new vscode.NotebookCellOutput([
		vscode.NotebookCellOutputItem.text(message, "text/plain"),
	]);
}

function errorOutput(message: string): vscode.NotebookCellOutput {
	return new vscode.NotebookCellOutput([
		vscode.NotebookCellOutputItem.error({ name: "Code Moniker", message }),
	]);
}
