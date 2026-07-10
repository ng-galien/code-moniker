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

import type { UnitPayload } from "../../protocol";
import { postFocus, postOpenSource } from "../actions";
import { FunctionCard } from "./FunctionCard";
import { layoutGraph } from "./layout";
import { buildUnitGraph, type GraphNodeModel } from "./model";

const NODE_TYPES = { functionCard: FunctionCard };

// The structural center: a top-down layered call DAG over the unit's
// callables. Clicking a card refocuses the explorer on that member; the
// right-click opens its source. Types stay off the canvas on a badge rail.
export function UnitGraph({ unit }: { unit: UnitPayload }) {
	const model = useMemo(() => buildUnitGraph(unit), [unit]);
	const [nodes, setNodes] = useState<Node[] | null>(null);

	useEffect(() => {
		let cancelled = false;
		setNodes(null);
		void layoutGraph(model.nodes, model.edges).then((positions) => {
			if (cancelled) {
				return;
			}
			setNodes(
				model.nodes.map((node) => ({
					id: node.symbol.id,
					type: "functionCard",
					position: positions.get(node.symbol.id) ?? { x: 0, y: 0 },
					data: { node },
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
				id: edge.id,
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
				<div className="muted graph-empty">no internal calls to draw</div>
			) : nodes == null ? (
				<div className="muted graph-empty">layout…</div>
			) : (
				<ReactFlow
				nodes={nodes}
				edges={edges}
				nodeTypes={NODE_TYPES}
				fitView
				minZoom={0.2}
				panOnScroll
				nodesConnectable={false}
				onNodeClick={(_, node) => postFocus(nodeSymbol(node).symbol.uri)}
				onNodeContextMenu={(event, node) => {
					event.preventDefault();
					postOpenSource(nodeSymbol(node).symbol);
				}}
				>
					<Background variant={BackgroundVariant.Dots} gap={18} size={1} />
				</ReactFlow>
			)}
			{(model.typeRail.length > 0 || model.hiddenEdges > 0) && (
				<div className="type-rail">
					{model.typeRail.map((symbol) => (
						<span
							key={symbol.uri}
							className="typechip"
							onClick={() => postFocus(symbol.uri)}
						>
							{symbol.name}
						</span>
					))}
					{model.hiddenEdges > 0 && (
						<span className="rail-note">{model.hiddenEdges} non-call edge(s) as badges</span>
					)}
				</div>
			)}
		</div>
	);
}

function nodeSymbol(node: Node): GraphNodeModel {
	return (node.data as { node: GraphNodeModel }).node;
}
