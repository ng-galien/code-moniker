import * as path from "node:path";
import * as vscode from "vscode";

export function workspaceLabel(uri: vscode.Uri): string {
	const folder = vscode.workspace.getWorkspaceFolder(uri);
	if (!folder) {
		return path.basename(uri.fsPath);
	}
	return path.relative(folder.uri.fsPath, uri.fsPath) || path.basename(uri.fsPath);
}

export function rootOf(uri: vscode.Uri, fallbackRoot?: string): string {
	const owningWorkspace = vscode.workspace.getWorkspaceFolder(uri);
	if (owningWorkspace) {
		return owningWorkspace.uri.fsPath;
	}
	if (uri.scheme !== "file") {
		const firstWorkspace = vscode.workspace.workspaceFolders?.[0];
		return firstWorkspace?.uri.fsPath ?? fallbackRoot ?? ".";
	}
	return path.dirname(uri.fsPath);
}

export function firstLine(text: string): string {
	return text.split("\n")[0];
}
