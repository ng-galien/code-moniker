import type {
	IdentityGraphEdge,
	IdentityGraphResult,
	IdentitySegmentDto,
	SymbolDto,
} from "../../../daemon/model";
import type { ContainerOutline, ScopeOutline } from "../../protocol";

// Relation hierarchy on the scoped canvas: calls always draw; instantiates
// and type usages hide behind toggles; whatever a rolled-up edge carries, it
// stays visible as long as one of its kinds is enabled.
export const CALL_KINDS = new Set(["calls", "method_call"]);

// Shared so a node without an outline yet keeps a stable identity across
// re-renders instead of invalidating the layout memo every time.
const NO_OUTLINE: ContainerOutline = { chain: [], members: [], hidden: 0 };

// One formula for the canvas edge key, shared by the layout request, the
// rendered edge and the click lookup.
export function edgeId(edge: IdentityGraphEdge): string {
	return `${edge.source}->${edge.target}`;
}

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
	// What the container holds: the flattened single-child path a dive would
	// traverse, and the members waiting at the landing.
	outline: ContainerOutline;
	callsIn: number;
	callsOut: number;
}

export interface ScopeGraphModel {
	nodes: ScopeNodeModel[];
	edges: IdentityGraphEdge[];
	byId: Map<string, IdentityGraphEdge>;
	hiddenEdges: number;
}

export function buildScopeGraph(
	graph: IdentityGraphResult,
	filters: ScopeFilters,
	outline: ScopeOutline,
): ScopeGraphModel {
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
		return {
			id: row.identity,
			def,
			row,
			outline: outline[row.identity] ?? NO_OUTLINE,
			callsIn,
			callsOut,
		};
	});
	const byId = new Map(edges.map((edge) => [edgeId(edge), edge]));
	return { nodes, edges, byId, hiddenEdges: graph.edges.length - edges.length };
}
