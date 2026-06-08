import * as vscode from "vscode";

import { Violation } from "../cli/model";

export function toDiagnostic(violation: Violation): vscode.Diagnostic {
	const [start, end] = violation.lines;
	const range = new vscode.Range(Math.max(0, start - 1), 0, Math.max(0, end - 1), 4096);
	const diag = new vscode.Diagnostic(
		range,
		violation.explanation ? `${violation.explanation}\n${violation.message}` : violation.message,
		violation.severity === "warn"
			? vscode.DiagnosticSeverity.Warning
			: vscode.DiagnosticSeverity.Error,
	);
	diag.source = "code-moniker";
	diag.code = violation.rule_id;
	return diag;
}
