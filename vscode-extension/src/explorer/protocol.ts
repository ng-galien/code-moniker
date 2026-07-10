import type { IdentityGraphResult } from "../daemon/model";

// Message contract between the explorer panel (extension host) and its
// webview. Types only — this module is imported from both sides of the
// bridge, so it must stay free of vscode and DOM value imports.
export interface ScopePayload {
	graph: IdentityGraphResult;
	canBack: boolean;
	canForward: boolean;
}

export interface ScopeMessage {
	type: "scope";
	payload: ScopePayload;
}

export interface ScopeErrorMessage {
	type: "scopeError";
	prefix: string;
	message: string;
}

export interface OpenSourceTarget {
	root: string;
	file: string;
	line: number;
}

export type ExplorerMessage =
	| { type: "focus"; prefix: string }
	| { type: "back" }
	| { type: "forward" }
	| { type: "openSource"; target: OpenSourceTarget }
	| { type: "ready" };
