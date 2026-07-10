import type { SymbolDto, SymbolGraphEdge } from "../../../daemon/model";
import type { UnitPayload } from "../../protocol";

// Projects the unit payload onto the call-first graph model: callable members
// become nodes, call-kind internal edges become arrows, type-shaped members
// fall back to a badge rail, and everything else stays countable but out of
// the picture. Relation hierarchy: calls are drawn, uses_type/reads never are.
export const CALL_KINDS = new Set(["calls", "method_call"]);

export const CALLABLE_KINDS = new Set(["fn", "function", "method", "test", "constructor", "macro"]);

const TYPE_KINDS = new Set(["struct", "enum", "trait", "type", "union", "interface", "class"]);

export interface GraphNodeModel {
	symbol: SymbolDto;
	entry: boolean;
	test: boolean;
	recursive: boolean;
	callsIn: number;
	callsOut: number;
}

export interface GraphEdgeModel {
	id: string;
	source: string;
	target: string;
	count: number;
	kinds: string[];
}

export interface UnitGraphModel {
	nodes: GraphNodeModel[];
	edges: GraphEdgeModel[];
	typeRail: SymbolDto[];
	hiddenEdges: number;
}

export function buildUnitGraph(unit: UnitPayload): UnitGraphModel {
	const focusUri = unit.focus.kind === "symbol" ? unit.focus.symbol.uri : null;
	const members = unit.members.filter((member) => member.uri !== focusUri);
	const callEdges = unit.internalEdges.filter((edge) =>
		edge.kinds.some((kind) => CALL_KINDS.has(kind)),
	);
	const hiddenEdges = unit.internalEdges.length - callEdges.length;

	const byId = new Map(members.map((member) => [member.id, member]));
	const inCalls = new Map<string, number>();
	const outCalls = new Map<string, number>();
	const recursive = new Set<string>();
	const edges: GraphEdgeModel[] = [];
	for (const edge of callEdges) {
		if (!byId.has(edge.source) || !byId.has(edge.target)) {
			continue;
		}
		if (edge.source === edge.target) {
			recursive.add(edge.source);
			continue;
		}
		outCalls.set(edge.source, (outCalls.get(edge.source) ?? 0) + edge.count);
		inCalls.set(edge.target, (inCalls.get(edge.target) ?? 0) + edge.count);
		edges.push({
			id: `${edge.source}->${edge.target}`,
			source: edge.source,
			target: edge.target,
			count: edge.count,
			kinds: edge.kinds,
		});
	}

	const nodes: GraphNodeModel[] = [];
	const typeRail: SymbolDto[] = [];
	for (const member of members) {
		const inEdges = inCalls.get(member.id) ?? 0;
		const outEdges = outCalls.get(member.id) ?? 0;
		const connected = inEdges > 0 || outEdges > 0 || recursive.has(member.id);
		if (CALLABLE_KINDS.has(member.kind) || (connected && !TYPE_KINDS.has(member.kind))) {
			nodes.push({
				symbol: member,
				entry: member.visibility === "public" && inEdges === 0,
				test: member.kind === "test",
				recursive: recursive.has(member.id),
				callsIn: inEdges,
				callsOut: outEdges,
			});
		} else if (TYPE_KINDS.has(member.kind)) {
			typeRail.push(member);
		}
	}

	const nodeIds = new Set(nodes.map((node) => node.symbol.id));
	return {
		nodes,
		edges: edges.filter((edge) => nodeIds.has(edge.source) && nodeIds.has(edge.target)),
		typeRail,
		hiddenEdges,
	};
}
