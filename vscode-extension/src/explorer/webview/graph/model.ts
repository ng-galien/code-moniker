import type {
	IdentityGraphEdge,
	IdentityGraphResult,
	IdentitySegmentDto,
	SymbolDto,
} from "../../../daemon/model";

// Relation hierarchy on the scoped canvas: calls always draw; instantiates
// and type usages hide behind toggles; whatever a rolled-up edge carries, it
// stays visible as long as one of its kinds is enabled.
export const CALL_KINDS = new Set(["calls", "method_call"]);

export interface ScopeFilters {
	instantiates: boolean;
	types: boolean;
}

export function edgeVisible(edge: IdentityGraphEdge, filters: ScopeFilters): boolean {
	return edge.kinds.some(
		(kind) =>
			CALL_KINDS.has(kind) ||
			(filters.instantiates && kind === "instantiates") ||
			(filters.types && (kind === "uses_type" || kind === "returns_type" || kind === "reads")),
	);
}

export interface GraphNodeModel {
	symbol: SymbolDto;
	entry: boolean;
	test: boolean;
	recursive: boolean;
	callsIn: number;
	callsOut: number;
}

export interface ScopeNodeModel {
	id: string;
	def?: GraphNodeModel;
	row: IdentitySegmentDto;
	callsIn: number;
	callsOut: number;
}

export interface ScopeGraphModel {
	nodes: ScopeNodeModel[];
	edges: IdentityGraphEdge[];
	hiddenEdges: number;
}

export function buildScopeGraph(graph: IdentityGraphResult, filters: ScopeFilters): ScopeGraphModel {
	const edges = graph.edges.filter((edge) => edgeVisible(edge, filters));
	const inbound = new Map<string, number>();
	const outbound = new Map<string, number>();
	for (const edge of edges) {
		outbound.set(edge.source, (outbound.get(edge.source) ?? 0) + edge.count);
		inbound.set(edge.target, (inbound.get(edge.target) ?? 0) + edge.count);
	}
	const nodes = graph.nodes.map((row) => {
		const callsIn = inbound.get(row.identity) ?? 0;
		const callsOut = outbound.get(row.identity) ?? 0;
		const def = row.symbol
			? {
					symbol: row.symbol,
					entry: row.symbol.visibility === "public" && callsIn === 0,
					test: row.symbol.kind === "test",
					recursive: false,
					callsIn,
					callsOut,
				}
			: undefined;
		return { id: row.identity, def, row, callsIn, callsOut };
	});
	return { nodes, edges, hiddenEdges: graph.edges.length - edges.length };
}

export function segmentName(identity: string): string {
	const segment = identity.split("/").pop() ?? identity;
	return segment.split(":")[1] ?? segment;
}
