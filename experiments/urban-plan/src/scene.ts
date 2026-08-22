/** Renderer-free urban-plan IR. Three.js must not live here. */

export type MetricBinding = "defs" | "none";

export type SceneBuilding = {
	id: string;
	label: string;
	kind: string;
	defs: number;
	hasChildren: boolean;
	u: number;
	v: number;
	width: number;
	depth: number;
	height: number;
};

export type SceneRoad = {
	from: string;
	to: string;
	count: number;
	kinds: string[];
};

export type SceneCoverage = {
	nodes: number;
	edgesTotal: number;
	edgesEmitted: number;
	roadsOmitted: number;
};

export type SceneSnapshot = {
	kind: "urban-plan";
	version: 1;
	generation: number | null;
	prefix: string;
	capturedAt: string;
	heightMetric: MetricBinding;
	footprintMetric: MetricBinding;
	buildings: SceneBuilding[];
	roads: SceneRoad[];
	coverage: SceneCoverage;
};

export type IdentityNode = {
	identity: string;
	kind: string;
	name: string;
	defs: number;
	has_children: boolean;
};

export type IdentityEdge = {
	source: string;
	target: string;
	kinds: string[];
	count: number;
};

export type CaptureInput = {
	generation: number | null;
	prefix: string;
	nodes: IdentityNode[];
	edges: IdentityEdge[];
	maxRoads?: number;
};

const HEIGHT_SCALE = 0.55;
const FOOTPRINT_SCALE = 0.22;
const CELL = 3.2;

export function snapshotFromIdentityGraph(input: CaptureInput): SceneSnapshot {
	const maxRoads = input.maxRoads ?? 24;
	const nodes = [...input.nodes].sort((a, b) =>
		a.identity.localeCompare(b.identity),
	);
	const columns = Math.max(1, Math.ceil(Math.sqrt(nodes.length)));
	const buildings: SceneBuilding[] = nodes.map((node, index) => {
		const col = index % columns;
		const row = Math.floor(index / columns);
		const span = 1 + Math.log1p(node.defs) * FOOTPRINT_SCALE;
		return {
			id: node.identity,
			label: node.name,
			kind: node.kind,
			defs: node.defs,
			hasChildren: node.has_children,
			u: col,
			v: row,
			width: span,
			depth: span,
			height: Math.max(0.4, Math.log1p(node.defs) * HEIGHT_SCALE),
		};
	});
	const known = new Set(buildings.map((building) => building.id));
	const ranked = [...input.edges]
		.filter((edge) => known.has(edge.source) && known.has(edge.target))
		.sort((a, b) => b.count - a.count);
	const roads: SceneRoad[] = ranked.slice(0, maxRoads).map((edge) => ({
		from: edge.source,
		to: edge.target,
		count: edge.count,
		kinds: edge.kinds,
	}));
	return {
		kind: "urban-plan",
		version: 1,
		generation: input.generation,
		prefix: input.prefix,
		capturedAt: new Date().toISOString(),
		heightMetric: "defs",
		footprintMetric: "defs",
		buildings,
		roads,
		coverage: {
			nodes: buildings.length,
			edgesTotal: ranked.length,
			edgesEmitted: roads.length,
			roadsOmitted: Math.max(0, ranked.length - roads.length),
		},
	};
}

export function buildingPosition(building: SceneBuilding): {
	x: number;
	z: number;
} {
	return { x: building.u * CELL, z: building.v * CELL };
}
