import * as vscode from "vscode";

import { toFsPath } from "../daemon/paths";
import { DetailWebview, SourceTarget } from "./detail/panel";
import { SymbolTreeNode } from "./nodes";
import { SymbolTreeProvider } from "./tree";

export function registerSymbolCommands(
	context: vscode.ExtensionContext,
	provider: SymbolTreeProvider,
	detail: DetailWebview,
): void {
	context.subscriptions.push(
		vscode.commands.registerCommand("codeMoniker.symbols.refresh", () => provider.refresh()),
		vscode.commands.registerCommand("codeMoniker.symbols.revealDetail", (node?: SymbolTreeNode) => {
			if (node?.kind === "symbol") {
				void detail.showForSymbol(node.symbol);
			}
		}),
		vscode.commands.registerCommand("codeMoniker.symbols.openSource", (arg: SymbolTreeNode | SourceTarget) =>
			openSource(arg),
		),
	);
}

async function openSource(arg: SymbolTreeNode | SourceTarget): Promise<void> {
	const target = normalizeTarget(arg);
	if (!target) {
		return;
	}
	const fsPath = toFsPath(target.root, target.file);
	const document = await vscode.workspace.openTextDocument(vscode.Uri.file(fsPath));
	const editor = await vscode.window.showTextDocument(document, {
		viewColumn: vscode.ViewColumn.One,
		preview: true,
	});
	const line = Math.max(0, target.line - 1);
	const position = new vscode.Position(line, 0);
	editor.selection = new vscode.Selection(position, position);
	editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);
}

function normalizeTarget(arg: SymbolTreeNode | SourceTarget): SourceTarget | undefined {
	if ("kind" in arg && arg.kind === "symbol") {
		return {
			root: arg.symbol.root,
			file: arg.symbol.file,
			line: arg.symbol.line_range ? arg.symbol.line_range[0] : 1,
		};
	}
	if ("file" in arg && "root" in arg) {
		return arg;
	}
	return undefined;
}
