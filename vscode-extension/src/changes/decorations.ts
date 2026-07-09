import * as vscode from "vscode";

import { ChangeReviewResult } from "../daemon/model";
import { toFsPath } from "../daemon/paths";
import { themeColor } from "../shared/appIcons";

// Projects the semantic change review onto explorer file badges: a `±` with
// the changed-symbol count, or `≠` when the file carries residual
// (unattributed) edits. Paths in the review are workspace-root relative, so
// each root produces one candidate URI.
export class ChangeDecorationModel implements vscode.FileDecorationProvider {
	private byAbsPath = new Map<string, { symbols: number; residual: boolean; disposition: string }>();

	private readonly emitter = new vscode.EventEmitter<vscode.Uri[] | undefined>();
	readonly onDidChangeFileDecorations = this.emitter.event;

	constructor(private readonly roots: string[]) {}

	update(review: ChangeReviewResult | undefined): void {
		const previous = [...this.byAbsPath.keys()];
		this.byAbsPath = new Map();
		for (const file of review?.files ?? []) {
			const path = file.new_path ?? file.old_path;
			if (!path) {
				continue;
			}
			for (const root of this.roots) {
				this.byAbsPath.set(toFsPath(root, path), {
					symbols: file.symbol_changes + file.moved_symbols,
					residual: !file.coverage_explained,
					disposition: file.disposition,
				});
			}
		}
		const affected = new Set<string>([...previous, ...this.byAbsPath.keys()]);
		this.emitter.fire([...affected].map((p) => vscode.Uri.file(p)));
	}

	provideFileDecoration(uri: vscode.Uri): vscode.FileDecoration | undefined {
		const entry = this.byAbsPath.get(uri.fsPath);
		if (!entry) {
			return undefined;
		}
		return new vscode.FileDecoration(
			entry.residual ? "≠" : "±",
			`${entry.disposition}: ${entry.symbols} symbol change(s)` +
				(entry.residual ? ", residual edits" : ""),
			themeColor(
				entry.residual
					? "gitDecoration.modifiedResourceForeground"
					: "gitDecoration.addedResourceForeground",
			),
		);
	}

	dispose(): void {
		this.emitter.dispose();
	}
}
