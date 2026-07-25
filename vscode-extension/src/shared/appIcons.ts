import * as vscode from "vscode";

export type CheckStatus = "error" | "pass" | "warning";

export type WorkspaceSection =
	| "setup"
	| "daemon"
	| "symbols"
	| "views"
	| "changes"
	| "check"
	| "ruleFiles";

export type CatalogGroup = "builtin" | "learn" | "language" | "rules";

// How a Setup row reads at a glance: configured, something the CLI tracks but
// can no longer find, not installed yet (a neutral state, not a warning), or
// unreachable.
export type SetupStatus = "ok" | "missing" | "absent" | "error";

function themed(id: string, color?: string): vscode.ThemeIcon {
	return color
		? new vscode.ThemeIcon(id, themeColor(color))
		: new vscode.ThemeIcon(id);
}

export function themeColor(id: string): vscode.ThemeColor {
	return new vscode.ThemeColor(id);
}

export function statusIcon(status: CheckStatus): vscode.ThemeIcon {
	if (status === "error") {
		return themed("error", "errorForeground");
	}
	if (status === "warning") {
		return themed("warning", "list.warningForeground");
	}
	return themed("pass", "testing.iconPassed");
}

export function infoIcon(): vscode.ThemeIcon {
	return themed("info", "charts.blue");
}

export function runningIcon(): vscode.ThemeIcon {
	return themed("loading~spin", "charts.blue");
}

export function daemonIcon(
	status?: "disconnected" | "ready" | "loading" | "connecting" | "error",
): vscode.ThemeIcon {
	if (status === "ready") {
		return themed("server-process", "testing.iconPassed");
	}
	if (status === "loading" || status === "connecting") {
		return runningIcon();
	}
	if (status === "error") {
		return themed("server-process", "errorForeground");
	}
	return themed("server-process", "charts.blue");
}

export function workspaceSectionIcon(id: WorkspaceSection): vscode.ThemeIcon {
	switch (id) {
		case "setup":
			return themed("gear", "charts.yellow");
		case "daemon":
			return themed("server-process", "charts.blue");
		case "symbols":
			return themed("symbol-misc", "charts.purple");
		case "views":
			return themed("references", "charts.purple");
		case "changes":
			return themed("git-compare", "charts.green");
		case "check":
			return themed("checklist", "charts.orange");
		case "ruleFiles":
			return themed("files", "charts.blue");
	}
}

export function setupStatusIcon(status: SetupStatus): vscode.ThemeIcon {
	switch (status) {
		case "ok":
			return themed("check", "charts.green");
		case "missing":
			return themed("warning", "charts.yellow");
		case "error":
			return themed("error", "charts.red");
		case "absent":
			return themed("circle-outline");
	}
}

export function setupComponentIcon(component: string): vscode.ThemeIcon {
	switch (component) {
		case "mcp":
			return themed("plug", "charts.blue");
		case "skill":
			return themed("book", "charts.purple");
		case "hooks":
			return themed("zap", "charts.orange");
		default:
			return themed("gear");
	}
}

export function changeSymbolIcon(kind: string): vscode.ThemeIcon {
	switch (kind) {
		case "added":
			return themed("diff-added", "gitDecoration.addedResourceForeground");
		case "removed":
			return themed("diff-removed", "gitDecoration.deletedResourceForeground");
		case "renamed":
		case "moved":
			return themed("diff-renamed", "gitDecoration.renamedResourceForeground");
		case "moved-and-modified":
			return themed("diff-renamed", "gitDecoration.modifiedResourceForeground");
		case "modified":
		case "body-modified":
		case "signature-changed":
		case "attribute-changed":
		default:
			return themed("diff-modified", "gitDecoration.modifiedResourceForeground");
	}
}

export function ruleFileIcon(): vscode.ThemeIcon {
	return themed("law", "charts.orange");
}

export function ruleIcon(severity?: string): vscode.ThemeIcon {
	return severity === "warn"
		? statusIcon("warning")
		: themed("shield", "charts.blue");
}

export function ruleFolderIcon(): vscode.ThemeIcon {
	return themed("folder", "charts.orange");
}

export function sourceFolderIcon(): vscode.ThemeIcon {
	return themed("folder", "charts.blue");
}

export function sourceFileIcon(): vscode.ThemeIcon {
	return vscode.ThemeIcon.File;
}

export function codeMonikerIcon(extensionUri: vscode.Uri): vscode.Uri {
	return vscode.Uri.joinPath(extensionUri, "icons", "activity.svg");
}

export function checkSectionIcon(): vscode.ThemeIcon {
	return themed("checklist", "charts.orange");
}

export function catalogGroupIcon(kind: CatalogGroup): vscode.ThemeIcon {
	if (kind === "language") {
		return themed("symbol-keyword", "charts.purple");
	}
	if (kind === "rules") {
		return ruleFileIcon();
	}
	if (kind === "learn") {
		return themed("book", "charts.green");
	}
	return themed("library", "charts.blue");
}

export function catalogEntryIcon(): vscode.ThemeIcon {
	return themed("notebook", "charts.green");
}

export function viewIcon(): vscode.ThemeIcon {
	return themed("references", "charts.purple");
}

export function boundaryIcon(): vscode.ThemeIcon {
	return themed("symbol-interface", "charts.blue");
}

export function gotchaIcon(): vscode.ThemeIcon {
	return statusIcon("warning");
}

export function evidenceIcon(): vscode.ThemeIcon {
	return themed("symbol-event", "charts.green");
}

export function symbolIcon(kind: string): vscode.ThemeIcon {
	const map: Record<string, { id: string; color: string }> = {
		function: { id: "symbol-function", color: "charts.green" },
		fn: { id: "symbol-function", color: "charts.green" },
		method: { id: "symbol-method", color: "charts.green" },
		struct: { id: "symbol-structure", color: "charts.blue" },
		class: { id: "symbol-class", color: "charts.blue" },
		interface: { id: "symbol-interface", color: "charts.purple" },
		trait: { id: "symbol-interface", color: "charts.purple" },
		enum: { id: "symbol-enum", color: "charts.orange" },
		field: { id: "symbol-field", color: "charts.yellow" },
		property: { id: "symbol-property", color: "charts.yellow" },
		constant: { id: "symbol-constant", color: "charts.orange" },
		const: { id: "symbol-constant", color: "charts.orange" },
		variable: { id: "symbol-variable", color: "charts.blue" },
		module: { id: "symbol-namespace", color: "charts.purple" },
		mod: { id: "symbol-namespace", color: "charts.purple" },
		namespace: { id: "symbol-namespace", color: "charts.purple" },
		type: { id: "symbol-type-parameter", color: "charts.purple" },
		impl: { id: "symbol-structure", color: "charts.blue" },
	};
	const icon = map[kind] ?? { id: "symbol-misc", color: "icon.foreground" };
	return themed(icon.id, icon.color);
}
