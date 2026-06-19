import * as vscode from "vscode";

import { toFsPath } from "../daemon/paths";
import { DaemonSession } from "../daemon/session";
import { DetailWebview, SourceTarget } from "./detail/panel";
import { SymbolTreeNode } from "./nodes";
import { SymbolTreeProvider } from "./tree";

export function registerSymbolCommands(
	context: vscode.ExtensionContext,
	session: DaemonSession,
	provider: SymbolTreeProvider,
	detail: DetailWebview,
): void {
	context.subscriptions.push(
		vscode.commands.registerCommand("codeMoniker.symbols.refresh", () =>
			refreshSymbols(session, provider),
		),
		vscode.commands.registerCommand("codeMoniker.symbols.revealDetail", (node?: SymbolTreeNode) => {
			const target = unwrapWorkspaceNode(node) as SymbolTreeNode | undefined;
			if (target?.kind === "symbol") {
				void detail.showForSymbol(target.symbol);
			}
		}),
		vscode.commands.registerCommand("codeMoniker.symbols.openSource", (arg: SymbolTreeNode | SourceTarget) =>
			openSource(arg),
		),
	);
}

async function refreshSymbols(
	session: DaemonSession,
	provider: SymbolTreeProvider,
): Promise<void> {
	if (!(await session.connectOrStart())) {
		return;
	}
	await session.refresh();
	provider.refresh();
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
	const targetArg = unwrapWorkspaceNode(arg) as SymbolTreeNode | SourceTarget;
	if ("kind" in targetArg && targetArg.kind === "symbol") {
		return {
			root: targetArg.symbol.root,
			file: targetArg.symbol.file,
			line: targetArg.symbol.line_range ? targetArg.symbol.line_range[0] : 1,
		};
	}
	if ("kind" in targetArg && targetArg.kind === "entry" && targetArg.tree.kind === "file") {
		return {
			root: targetArg.tree.root,
			file: targetArg.tree.path,
			line: 1,
		};
	}
	if ("file" in targetArg && "root" in targetArg) {
		return targetArg;
	}
	return undefined;
}

function unwrapWorkspaceNode(arg: unknown): unknown {
	if (arg && typeof arg === "object" && "node" in arg) {
		return (arg as { node?: unknown }).node;
	}
	return arg;
}
