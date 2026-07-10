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
import { postFocus, postOpenSource } from "./actions";
import { ContainerCard } from "./graph/ContainerCard";
import { FunctionCard } from "./graph/FunctionCard";
import { layoutGraph } from "./graph/layout";
import { buildScopeGraph, type ScopeFilters, type ScopeNodeModel } from "./graph/model";

const NODE_TYPES = { functionCard: FunctionCard, containerCard: ContainerCard };

// The scoped canvas: the prefix's children as cards, rolled-up references as
// weighted edges. Double-click dives into a node; right-click opens source
// for definitions; clicking an edge opens its facts in the side panel.
export function ScopeCanvas({
	graph,
	filters,
	onSelectEdge,
}: {
	graph: IdentityGraphResult;
	filters: ScopeFilters;
	onSelectEdge: (edge: IdentityGraphEdge | null) => void;
}) {
	const model = useMemo(() => buildScopeGraph(graph, filters), [graph, filters]);
	const [nodes, setNodes] = useState<Node[] | null>(null);

	useEffect(() => {
		let cancelled = false;
		setNodes(null);
		const edgeRefs = model.edges.map((edge) => ({
			id: `${edge.source}->${edge.target}`,
			source: edge.source,
			target: edge.target,
		}));
		void layoutGraph(
			model.nodes.map((node) => node.id),
			edgeRefs,
		).then((positions) => {
			if (cancelled) {
				return;
			}
			setNodes(
				model.nodes.map((node) => ({
					id: node.id,
					type: node.def ? "functionCard" : "containerCard",
					position: positions.get(node.id) ?? { x: 0, y: 0 },
					data: node.def ? { node: node.def } : { node },
				})),
			);
		});
		return () => {
			cancelled = true;
		};
	}, [model]);

	const edges = useMemo<Edge[]>(
		() =>
			model.edges.map((edge) => ({
				id: `${edge.source}->${edge.target}`,
				source: edge.source,
				target: edge.target,
				type: "smoothstep",
				className: "call-edge",
				label: edge.count > 1 ? `×${edge.count}` : undefined,
				style: { strokeWidth: 1 + Math.log2(edge.count) },
				markerEnd: { type: MarkerType.ArrowClosed },
			})),
		[model],
	);

	return (
		<div className="unit-graph">
			{model.nodes.length === 0 ? (
				<div className="muted graph-empty">
					This scope has no members to draw. Press Backspace to climb up a level.
				</div>
			) : nodes == null ? (
				<div className="muted graph-empty">Laying out…</div>
			) : (
				<ReactFlow
					nodes={nodes}
					edges={edges}
					nodeTypes={NODE_TYPES}
					fitView
					minZoom={0.2}
					panOnScroll
					nodesConnectable={false}
					onNodeDoubleClick={(_, node) => postFocus(node.id)}
					onNodeContextMenu={(event, node) => {
						event.preventDefault();
						const model = nodeModel(node);
						if (model.def) {
							postOpenSource(model.def.symbol);
						}
					}}
					onEdgeClick={(_, edge) => {
						const found = model.edges.find(
							(candidate) => `${candidate.source}->${candidate.target}` === edge.id,
						);
						onSelectEdge(found ?? null);
					}}
					onPaneClick={() => onSelectEdge(null)}
				>
					<Background variant={BackgroundVariant.Dots} gap={18} size={1} />
				</ReactFlow>
			)}
			{(graph.ports_in.length > 0 || graph.ports_out.length > 0 || model.hiddenEdges > 0) && (
				<div className="port-rail">
					{graph.ports_in.length > 0 && (
						<span className="port-rail-group">
							<span className="port-rail-label">from outside</span>
							{graph.ports_in.map((port) => (
								<button
									key={`in:${port.identity}`}
									type="button"
									className="portchip in"
									title={`${port.identity} — ${port.kinds.join(", ")}`}
									onClick={() => postFocus(port.identity)}
								>
									⟵ {shortIdentity(port.identity)} ×{port.count}
								</button>
							))}
						</span>
					)}
					{graph.ports_out.length > 0 && (
						<span className="port-rail-group">
							<span className="port-rail-label">to outside</span>
							{graph.ports_out.map((port) => (
								<button
									key={`out:${port.identity}`}
									type="button"
									className="portchip out"
									title={`${port.identity} — ${port.kinds.join(", ")}`}
									onClick={() => postFocus(port.identity)}
								>
									⟶ {shortIdentity(port.identity)} ×{port.count}
								</button>
							))}
						</span>
					)}
					{model.hiddenEdges > 0 && (
						<span className="rail-note">{model.hiddenEdges} edge(s) hidden by filters</span>
					)}
				</div>
			)}
		</div>
	);
}

function nodeModel(node: Node): ScopeNodeModel {
	const data = node.data as { node: ScopeNodeModel };
	return data.node;
}

function shortIdentity(identity: string): string {
	const segment = identity.split("/").pop() ?? identity;
	return segment.split(":")[1] ?? segment;
}
