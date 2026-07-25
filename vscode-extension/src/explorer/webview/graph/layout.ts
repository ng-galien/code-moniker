import ELK, { type ElkNode } from "elkjs/lib/elk.bundled.js";

// Layered layout, top-down: entry points end up on the first rank because
// they have no incoming edge; ELK breaks cycles on its own. Positions and
// edge routes are computed off-DOM from declared card sizes.
export interface Point {
	x: number;
	y: number;
}

export interface LayoutBox {
	id: string;
	width: number;
	height: number;
}

export interface GraphLayout {
	nodes: Map<string, Point>;
	// Polyline per edge id, source border to target border, including the
	// bends ELK routed around the cards. Empty when ELK returned no section.
	routes: Map<string, Point[]>;
}

const elk = new ELK();

export interface CardMetrics {
	width: number;
	height: number;
	memberChrome: number;
	memberRow: number;
	memberMore: number;
}

let cached: CardMetrics | undefined;

// The card geometry lives in explorer.css as custom properties; reading it
// once here keeps ELK's reserved boxes and the rendered cards on one source
// instead of two sets of numbers that must be edited together.
export function cardMetrics(): CardMetrics {
	if (!cached) {
		const style = getComputedStyle(document.documentElement);
		const px = (name: string, fallback: number): number => {
			const value = Number.parseFloat(style.getPropertyValue(name));
			return Number.isFinite(value) && value > 0 ? value : fallback;
		};
		cached = {
			width: px("--cm-card-width", 190),
			height: px("--cm-card-height", 64),
			memberChrome: px("--cm-member-chrome", 20),
			memberRow: px("--cm-member-row", 16),
			memberMore: px("--cm-member-more", 15),
		};
	}
	return cached;
}

export async function layoutGraph(
	boxes: LayoutBox[],
	edges: { id: string; source: string; target: string }[],
): Promise<GraphLayout> {
	// Typed as ElkNode so the result carries ELK's own shapes (edge sections),
	// not the narrow literal we passed in.
	const graph: ElkNode = {
		id: "scope",
		layoutOptions: {
			"elk.algorithm": "layered",
			"elk.direction": "DOWN",
			// Orthogonal routing keeps every edge on its own lane and bends
			// around the cards. Bezier curves crossing a dense level read as
			// spaghetti; right angles stay followable.
			"elk.edgeRouting": "ORTHOGONAL",
			"elk.layered.spacing.nodeNodeBetweenLayers": "76",
			"elk.spacing.nodeNode": "40",
			"elk.spacing.edgeNode": "22",
			"elk.spacing.edgeEdge": "14",
			"elk.layered.spacing.edgeNodeBetweenLayers": "26",
			"elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
			"elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
		},
		children: boxes.map((box) => ({
			id: box.id,
			width: box.width,
			height: box.height,
		})),
		edges: edges.map((edge) => ({
			id: edge.id,
			sources: [edge.source],
			targets: [edge.target],
		})),
	};
	const result = await elk.layout(graph);
	const nodes = new Map<string, Point>();
	for (const child of result.children ?? []) {
		nodes.set(child.id, { x: child.x ?? 0, y: child.y ?? 0 });
	}
	const routes = new Map<string, Point[]>();
	for (const edge of result.edges ?? []) {
		const section = edge.sections?.[0];
		if (!section) {
			continue;
		}
		routes.set(edge.id, [
			section.startPoint,
			...(section.bendPoints ?? []),
			section.endPoint,
		]);
	}
	return { nodes, routes };
}
