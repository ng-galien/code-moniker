import {
	Background,
	BackgroundVariant,
	MarkerType,
	ReactFlow,
	type Edge,
	type Node,
} from "@xyflow/react";
import { useEffect, useMemo, useState } from "react";

import "@xyflow/react/dist/style.css";

import type { IdentityGraphEdge, IdentityGraphResult } from "../../daemon/model";
import type { ScopeOutline } from "../protocol";
import { segmentName } from "../../shared/identity";
import { postFocus, postOpenSource } from "./actions";
import { ContainerCard } from "./graph/ContainerCard";
import { FunctionCard } from "./graph/FunctionCard";
import { RoutedEdge } from "./graph/RoutedEdge";
import { cardMetrics, layoutGraph, type LayoutBox } from "./graph/layout";
import { buildScopeGraph, edgeId, type ScopeFilters, type ScopeNodeModel } from "./graph/model";

// Every canvas node carries the same data shape ({ node: ScopeNodeModel });
// the def card unwraps it here. Click handlers and cards reading one shape is
// what keeps "renders fine but clicks dead" drifts impossible.
function ScopeFunctionCard({ data }: { data: { node: ScopeNodeModel } }) {
	if (!data.node.def) {
		return null;
	}
	return <FunctionCard data={{ node: data.node.def }} />;
}

const NODE_TYPES = { functionCard: ScopeFunctionCard, containerCard: ContainerCard };
const EDGE_TYPES = { routed: RoutedEdge };

// The scoped canvas: the prefix's children as cards, rolled-up references as
// weighted edges routed by ELK. Double-click dives into a node; double-click
// on the empty background climbs one level; right-click opens source for
// definitions; clicking an edge opens its facts panel.
export function ScopeCanvas({
	graph,
	filters,
	outline,
	onSelectEdge,
	onClimb,
	onInspect,
}: {
	graph: IdentityGraphResult;
	filters: ScopeFilters;
	outline: ScopeOutline;
	onSelectEdge: (edge: IdentityGraphEdge | null) => void;
	onClimb: () => void;
	onInspect: (uri: string) => void;
}) {
	const model = useMemo(() => buildScopeGraph(graph, filters, outline), [graph, filters, outline]);
	const [laidOut, setLaidOut] = useState<{ nodes: Node[]; edges: Edge[] } | null>(null);

	useEffect(() => {
		let cancelled = false;
		setLaidOut(null);
		const edgeRefs = model.edges.map((edge) => ({
			id: edgeId(edge),
			source: edge.source,
			target: edge.target,
		}));
		void layoutGraph(model.nodes.map(boxFor), edgeRefs).then((layout) => {
			if (cancelled) {
				return;
			}
			setLaidOut({
				nodes: model.nodes.map((node) => ({
					id: node.id,
					type: node.def ? "functionCard" : "containerCard",
					position: layout.nodes.get(node.id) ?? { x: 0, y: 0 },
					data: { node },
				})),
				edges: model.edges.map((edge) => {
					const id = edgeId(edge);
					return {
						id,
						source: edge.source,
						target: edge.target,
						type: "routed",
						className: "call-edge",
						data: { points: layout.routes.get(id) ?? [], count: edge.count },
						style: { strokeWidth: edgeWidth(edge.count) },
						// The default marker scales with the stroke (markerUnits =
						// strokeWidth): on a heavy rollup it becomes a giant
						// triangle. Fixed user-space size keeps it an arrowhead.
						markerEnd: {
							type: MarkerType.ArrowClosed,
							width: 13,
							height: 13,
							markerUnits: "userSpaceOnUse",
							color: "var(--cm-accent)",
						},
					};
				}),
			});
		});
		return () => {
			cancelled = true;
		};
	}, [model]);

	return (
		<div className="unit-graph">
			{model.nodes.length === 0 ? (
				<div className="muted graph-empty">
					This scope has no members to draw. Press Backspace to climb up a level.
				</div>
			) : laidOut == null ? (
				<div className="muted graph-empty">Laying out…</div>
			) : (
				<ReactFlow
					nodes={laidOut.nodes}
					edges={laidOut.edges}
					nodeTypes={NODE_TYPES}
					edgeTypes={EDGE_TYPES}
					fitView
					minZoom={0.2}
					panOnScroll
					zoomOnDoubleClick={false}
					nodesConnectable={false}
					// Routes are computed by ELK against these positions: a
					// dragged card would leave its edges behind.
					nodesDraggable={false}
					onNodeDoubleClick={(_, node) => postFocus(node.id)}
					onNodeClick={(_, node) => {
						const model = nodeModel(node);
						if (model.def) {
							onInspect(model.def.symbol.uri);
						}
					}}
					onNodeContextMenu={(event, node) => {
						event.preventDefault();
						const model = nodeModel(node);
						if (model.def) {
							postOpenSource(model.def.symbol);
						}
					}}
					onEdgeClick={(_, edge) => onSelectEdge(model.byId.get(edge.id) ?? null)}
					onPaneClick={(event) => {
						if (event.detail === 2) {
							onClimb();
						} else {
							onSelectEdge(null);
						}
					}}
				>
					<Background variant={BackgroundVariant.Dots} gap={18} size={1} />
				</ReactFlow>
			)}
			{(graph.ports_in.length > 0 || graph.ports_out.length > 0 || model.hiddenEdges > 0) && (
				<div className="port-rail">
					{graph.ports_in.map((port) => (
						<button
							key={`in:${port.identity}`}
							type="button"
							className="portchip in"
							title={`Used from outside: ${port.identity} — ${port.kinds.join(", ")}`}
							onClick={() => postFocus(port.identity)}
						>
							⟵ {segmentName(port.identity)} ×{port.count}
						</button>
					))}
					{graph.ports_out.map((port) => (
						<button
							key={`out:${port.identity}`}
							type="button"
							className="portchip out"
							title={`Reaches outside: ${port.identity} — ${port.kinds.join(", ")}`}
							onClick={() => postFocus(port.identity)}
						>
							⟶ {segmentName(port.identity)} ×{port.count}
						</button>
					))}
					{model.hiddenEdges > 0 && (
						<span className="rail-note" title="Edges hidden by the relation filters">
							+{model.hiddenEdges} hidden
						</span>
					)}
				</div>
			)}
		</div>
	);
}

// Containers grow with the members they list, so ELK reserves the room the
// card actually needs instead of overlapping the rank below.
function boxFor(node: ScopeNodeModel): LayoutBox {
	const metrics = cardMetrics();
	const { members, hidden } = node.outline;
	const list = members.length
		? metrics.memberChrome +
			members.length * metrics.memberRow +
			(hidden > 0 ? metrics.memberMore : 0)
		: 0;
	return { id: node.id, width: metrics.width, height: metrics.height + list };
}

// Sub-linear width: the eye compares "thin / medium / heavy", not 179 vs 84.
// Uncapped log2 turned heavy rollups into 9px pipes that dwarfed the cards.
function edgeWidth(count: number): number {
	return Math.min(1.1 + Math.log2(count) * 0.3, 3.4);
}

function nodeModel(node: Node): ScopeNodeModel {
	const data = node.data as { node: ScopeNodeModel };
	return data.node;
}
