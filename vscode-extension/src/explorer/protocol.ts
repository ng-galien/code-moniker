import type { SymbolDto, SymbolGraphResult } from "../daemon/model";
import type { HighlightedSourceSnippet } from "../symbols/detail/highlight";

// Message contract between the explorer panel (extension host) and its
// webview. Types only — this module is imported from both sides of the
// bridge, so it must stay free of vscode and DOM value imports.

// Symbol-centered cockpit. Unlike a scope graph, this is an ego graph: the
// focused definition sits in the middle, callers arrive from the left and
// dependencies leave to the right. The webview deliberately reveals only a
// small top-N and keeps the remainder behind explicit expansion controls.
export interface CockpitPayload {
	graph: SymbolGraphResult;
	canBack: boolean;
	canForward: boolean;
	pinned: SymbolDto[];
	preferences: CockpitPreferences;
	context?: CockpitContext;
	perspectives: CockpitSavedPerspective[];
}

export interface CockpitContext {
	identity: string;
	label: string;
	kind: string;
}

export type CockpitPerspective = "neighborhood" | "impact";
export type CockpitRelation = "calls" | "data" | "types" | "references";

export interface CockpitFilters {
	incoming: boolean;
	outgoing: boolean;
	calls: boolean;
	data: boolean;
	types: boolean;
	references: boolean;
}

export interface CockpitPreferences {
	perspective: CockpitPerspective;
	filters: CockpitFilters;
	radius: CockpitRadius;
	positions: Record<string, CockpitPosition>;
}

export interface CockpitRadius {
	incoming: number;
	outgoing: number;
}

export interface CockpitPosition {
	x: number;
	y: number;
}

export interface CockpitViewport extends CockpitPosition {
	zoom: number;
}

export interface CockpitSavedPerspective {
	name: string;
	focus: string;
	pinnedUris: string[];
	preferences: CockpitPreferences;
}

export interface CockpitMessage {
	type: "cockpit";
	payload: CockpitPayload;
}

export interface CockpitEmptyMessage {
	type: "cockpitEmpty";
	context?: CockpitContext;
}

export interface CockpitLoadingMessage {
	type: "cockpitLoading";
	prefix: string;
}

export interface CockpitExpansionMessage {
	type: "cockpitExpansion";
	uri: string;
	requestId: string;
	rootFocus: string;
	generation: number;
	graph: SymbolGraphResult;
}

export interface CockpitExpansionErrorMessage {
	type: "cockpitExpansionError";
	uri: string;
	requestId: string;
	rootFocus: string;
	generation: number;
	message: string;
}

export interface SearchResultsMessage {
	type: "searchResults";
	query: string;
	rows: SymbolDto[];
}

export interface ExternalSelectionMessage {
	type: "externalSelection";
	symbol: SymbolDto;
	source: "tree" | "editor";
}

export type ExplorerStateMessage = CockpitEmptyMessage | CockpitLoadingMessage | CockpitMessage | CockpitErrorMessage;

export interface CockpitErrorMessage {
	type: "cockpitError";
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

// The webview acknowledges every cockpit it applies. DOM evidence keeps the
// integration test honest about React Flow's mounted edges and controls.
export interface CockpitAck {
	prefix: string;
	nodes: number;
	edges?: number;
	mode: "cockpit";
	perspective?: "neighborhood" | "impact";
	radius?: CockpitRadius;
	enabledRelations?: string[];
	pins?: number;
	framedNodes?: number;
	viewportZoom?: number;
	mountedEdgePaths?: number;
	visibleEdgePaths?: number;
	paintedEdgePaths?: number;
	zoomControls?: number;
	reactFlowReady?: boolean;
	viewport?: CockpitViewport;
	viewportCommandId?: number;
}

// Posted by the webview after it renders a code inset: `lines` counts the
// highlighted source lines actually shown (0 = "no source zone" fallback).
export interface InsetAck {
	uri: string;
	lines: number;
	reason?: "loaded" | "preserved";
	inspectorMode?: "contextual";
	graphMounted?: boolean;
	inspectorMounted?: boolean;
	legacyPathPickerPresent?: boolean;
}

export type ExplorerMessage =
	| { type: "focus"; prefix: string }
	| {
		type: "expand";
		uri: string;
		requestId: string;
		rootFocus: string;
		generation: number;
	}
	| { type: "search"; query: string }
	| { type: "pin"; uri: string; pinned: boolean }
	| { type: "preferences"; preferences: CockpitPreferences }
	| { type: "savePerspective" }
	| { type: "loadPerspective"; name: string }
	| { type: "deletePerspective"; name: string }
	| { type: "back" }
	| { type: "forward" }
	| { type: "inspect"; uri: string }
	| { type: "openSource"; target: OpenSourceTarget }
	| { type: "ready" }
	| ({ type: "ack" } & CockpitAck)
	| ({ type: "insetAck" } & InsetAck);
