import type { IdentityGraphResult, SymbolDto } from "../daemon/model";
import type { HighlightedSourceSnippet } from "../symbols/detail/highlight";

// Message contract between the explorer panel (extension host) and its
// webview. Types only — this module is imported from both sides of the
// bridge, so it must stay free of vscode and DOM value imports.

// What a container node holds, so its card shows the contents instead of
// forcing a dive: the flattened single-child path the dive would traverse,
// a preview of the members found there, and how many exist in total.
export interface MemberPreview {
	identity: string;
	name: string;
	kind: string;
}

export interface ContainerOutline {
	chain: string[];
	members: MemberPreview[];
	hidden: number;
}

export type ScopeOutline = Record<string, ContainerOutline>;

export interface ScopePayload {
	graph: IdentityGraphResult;
	canBack: boolean;
	canForward: boolean;
	outline: ScopeOutline;
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

// The webview acknowledges every scope it applies. This closes the loop for
// the e2e suite: an ack proves the React bundle loaded, received the message
// and rendered the level — not merely that the host posted it.
export interface ScopeAck {
	prefix: string;
	nodes: number;
}

// Posted by the webview after it renders a code inset: `lines` counts the
// highlighted source lines actually shown (0 = "no source zone" fallback).
export interface InsetAck {
	uri: string;
	lines: number;
}

export type ExplorerMessage =
	| { type: "focus"; prefix: string }
	| { type: "back" }
	| { type: "forward" }
	| { type: "inspect"; uri: string }
	| { type: "openSource"; target: OpenSourceTarget }
	| { type: "ready" }
	| ({ type: "ack" } & ScopeAck)
	| ({ type: "insetAck" } & InsetAck);
