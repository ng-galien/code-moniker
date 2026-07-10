import type {
	SymbolDto,
	SymbolGraphEdge,
	SymbolGraphFocus,
	SymbolGraphNeighbor,
} from "../daemon/model";
import type { HighlightedSourceSnippet } from "../symbols/detail/highlight";

// Message contract between the explorer panel (extension host) and its
// webview. Types only — this module is imported from both sides of the
// bridge, so it must stay free of vscode and DOM value imports.
export interface UnitPayload {
	focus: SymbolGraphFocus;
	members: SymbolDto[];
	internalEdges: SymbolGraphEdge[];
	callers: SymbolGraphNeighbor[];
	callees: SymbolGraphNeighbor[];
	unresolvedRefs: number;
	source: HighlightedSourceSnippet | null;
	canBack: boolean;
	canForward: boolean;
}

export interface UnitMessage {
	type: "unit";
	payload: UnitPayload;
}

export interface OpenSourceTarget {
	root: string;
	file: string;
	line: number;
}

export type ExplorerMessage =
	| { type: "focus"; uri: string }
	| { type: "back" }
	| { type: "forward" }
	| { type: "openSource"; target: OpenSourceTarget }
	| { type: "ready" };
