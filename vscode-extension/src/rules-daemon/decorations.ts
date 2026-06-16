import * as vscode from "vscode";

import { LineRange, SymbolDto, ViolationDto } from "../daemon/model";
import { toFsPath, toRelative } from "../daemon/paths";
import { ViolationIndex } from "../symbols/tree";

// Holds the latest check violations and projects them three ways: symbol-tree
// overlay (ViolationIndex), file badges (FileDecorationProvider), and the Problems
// panel (DiagnosticCollection). All lookup maps are built once per update.
export class ViolationModel implements ViolationIndex, vscode.FileDecorationProvider {
	private byRelPath = new Map<string, number>();
	private byAbsPath = new Map<string, number>();
	private rangesByRelPath = new Map<string, LineRange[]>();

	private readonly decorationEmitter = new vscode.EventEmitter<vscode.Uri[] | undefined>();
	readonly onDidChangeFileDecorations = this.decorationEmitter.event;

	constructor(private readonly diagnostics: vscode.DiagnosticCollection) {}

	update(violations: ViolationDto[]): void {
		const previous = [...this.byAbsPath.keys()];
		this.byRelPath = new Map();
		this.byAbsPath = new Map();
		this.rangesByRelPath = new Map();
		for (const violation of violations) {
			bump(this.byRelPath, toRelative(violation.root, violation.path));
			bump(this.byAbsPath, toFsPath(violation.root, violation.path));
			push(this.rangesByRelPath, toRelative(violation.root, violation.path), violation.lines);
		}
		this.publishDiagnostics(violations);
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
		const ranges = this.rangesByRelPath.get(symbol.file) ?? [];
		return ranges.filter(([vs, ve]) => vs <= end && ve >= start).length;
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

	private publishDiagnostics(violations: ViolationDto[]): void {
		this.diagnostics.clear();
		const byFile = new Map<string, vscode.Diagnostic[]>();
		for (const violation of violations) {
			const key = toFsPath(violation.root, violation.path);
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

function bump(map: Map<string, number>, key: string): void {
	map.set(key, (map.get(key) ?? 0) + 1);
}

function push(map: Map<string, LineRange[]>, key: string, value: LineRange): void {
	const list = map.get(key);
	if (list) {
		list.push(value);
	} else {
		map.set(key, [value]);
	}
}
