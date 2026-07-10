import type { IdentityGraphResult, SymbolDto } from "../daemon/model";
import type { HighlightedSourceSnippet } from "../symbols/detail/highlight";

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

// A code inset: the zone of one definition (its lines plus a little
// context), highlighted host-side — never the whole file.
export interface InsetMessage {
	type: "inset";
	uri: string;
	symbol: SymbolDto;
	source: HighlightedSourceSnippet | null;
}

export type ExplorerMessage =
	| { type: "focus"; prefix: string }
	| { type: "back" }
	| { type: "forward" }
	| { type: "inspect"; uri: string }
	| { type: "openSource"; target: OpenSourceTarget }
	| { type: "ready" };
