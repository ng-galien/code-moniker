import ELK from "elkjs/lib/elk.bundled.js";

import type { GraphEdgeModel, GraphNodeModel } from "./model";

// Layered layout, top-down: entry points end up on the first rank because
// they have no incoming call edge; ELK breaks the cycles (re-entry waves)
// on its own. Positions are computed off-DOM from estimated card sizes.
export interface PositionedNode {
	id: string;
	x: number;
	y: number;
	width: number;
	height: number;
}

const elk = new ELK();

export const CARD_WIDTH = 190;
export const CARD_HEIGHT = 64;

export async function layoutGraph(
	nodes: GraphNodeModel[],
	edges: GraphEdgeModel[],
): Promise<Map<string, PositionedNode>> {
	const graph = {
		id: "unit",
		layoutOptions: {
			"elk.algorithm": "layered",
			"elk.direction": "DOWN",
			"elk.layered.spacing.nodeNodeBetweenLayers": "56",
			"elk.spacing.nodeNode": "28",
			"elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
			"elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
		},
		children: nodes.map((node) => ({
			id: node.symbol.id,
			width: CARD_WIDTH,
			height: CARD_HEIGHT,
		})),
		edges: edges.map((edge) => ({
			id: edge.id,
			sources: [edge.source],
			targets: [edge.target],
		})),
	};
	const result = await elk.layout(graph);
	const positions = new Map<string, PositionedNode>();
	for (const child of result.children ?? []) {
		positions.set(child.id, {
			id: child.id,
			x: child.x ?? 0,
			y: child.y ?? 0,
			width: child.width ?? CARD_WIDTH,
			height: child.height ?? CARD_HEIGHT,
		});
	}
	return positions;
}
