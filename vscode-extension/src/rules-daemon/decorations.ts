import * as path from "node:path";
import * as vscode from "vscode";

import { SymbolDto, ViolationDto } from "../daemon/model";
import { ViolationIndex } from "../symbols/tree";

// Holds the latest check violations and projects them three ways: symbol-tree
// overlay (ViolationIndex), file badges (FileDecorationProvider), and the Problems
// panel (DiagnosticCollection).
export class ViolationModel implements ViolationIndex, vscode.FileDecorationProvider {
	private violations: ViolationDto[] = [];
	private byRelPath = new Map<string, number>();
	private byAbsPath = new Map<string, number>();

	private readonly decorationEmitter = new vscode.EventEmitter<vscode.Uri[] | undefined>();
	readonly onDidChangeFileDecorations = this.decorationEmitter.event;

	constructor(private readonly diagnostics: vscode.DiagnosticCollection) {}

	update(violations: ViolationDto[]): void {
		const previous = [...this.byAbsPath.keys()];
		this.violations = violations;
		this.byRelPath = countBy(violations, violationRelPath);
		this.byAbsPath = countBy(violations, (v) => absPath(v));
		this.publishDiagnostics();
		const affected = new Set<string>([...previous, ...this.byAbsPath.keys()]);
		this.decorationEmitter.fire([...affected].map((p) => vscode.Uri.file(p)));
	}

	clear(): void {
		this.update([]);
	}

	fileViolations(filePath: string): number {
		return this.byRelPath.get(filePath) ?? 0;
	}

	symbolViolations(symbol: SymbolDto): number {
		if (!symbol.line_range) {
			return 0;
		}
		const [start, end] = symbol.line_range;
		return this.violations.filter(
			(v) => violationRelPath(v) === symbol.file && v.lines[0] <= end && v.lines[1] >= start,
		).length;
	}

	provideFileDecoration(uri: vscode.Uri): vscode.FileDecoration | undefined {
		const count = this.byAbsPath.get(uri.fsPath);
		if (!count) {
			return undefined;
		}
		return new vscode.FileDecoration(
			"⚠",
			`${count} code-moniker violation(s)`,
			new vscode.ThemeColor("list.warningForeground"),
		);
	}

	private publishDiagnostics(): void {
		this.diagnostics.clear();
		const byFile = new Map<string, vscode.Diagnostic[]>();
		for (const violation of this.violations) {
			const key = absPath(violation);
			const list = byFile.get(key) ?? [];
			list.push(toDiagnostic(violation));
			byFile.set(key, list);
		}
		for (const [file, list] of byFile) {
			this.diagnostics.set(vscode.Uri.file(file), list);
		}
	}

	dispose(): void {
		this.decorationEmitter.dispose();
	}
}

function toDiagnostic(violation: ViolationDto): vscode.Diagnostic {
	const start = Math.max(0, violation.lines[0] - 1);
	const end = Math.max(start, violation.lines[1] - 1);
	const range = new vscode.Range(start, 0, end, Number.MAX_SAFE_INTEGER);
	const severity = violation.severity === "warn"
		? vscode.DiagnosticSeverity.Warning
		: vscode.DiagnosticSeverity.Error;
	const diagnostic = new vscode.Diagnostic(range, violation.message, severity);
	diagnostic.code = violation.rule_id;
	diagnostic.source = "code-moniker";
	return diagnostic;
}

function absPath(violation: ViolationDto): string {
	return path.isAbsolute(violation.path)
		? violation.path
		: path.join(violation.root, violation.path);
}

// The daemon reports violation paths as absolute; the symbol tree and SymbolDto use
// workspace-relative paths, so normalise to relative for cross-referencing.
export function violationRelPath(violation: ViolationDto): string {
	return path.isAbsolute(violation.path)
		? path.relative(violation.root, violation.path)
		: violation.path;
}

function countBy<T>(items: T[], key: (item: T) => string): Map<string, number> {
	const map = new Map<string, number>();
	for (const item of items) {
		const k = key(item);
		map.set(k, (map.get(k) ?? 0) + 1);
	}
	return map;
}
