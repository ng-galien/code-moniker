import { DEFAULT_LAYOUT, type LayoutConfig } from "./config.ts";
import { routeAlongStreets } from "./route.ts";
import type {
	IdentityEdge,
	IdentityTree,
	RoadClass,
	SceneBuilding,
	SceneDistrict,
	SceneRoad,
	SceneSnapshot,
	SceneStreet,
} from "./scene.ts";

type Rect = { x: number; z: number; w: number; d: number };

const CRATE_COLORS = [
	"#4a8aa0",
	"#c4a05a",
	"#4a9a7a",
	"#c46a78",
	"#6a6ab8",
	"#5a9a58",
	"#c48a58",
	"#4a7a98",
];

const FOLDER_FLAGS: Record<string, keyof Pick<LayoutConfig, "includeSrc" | "includeTests" | "includeBenches" | "includeExamples">> = {
	src: "includeSrc",
	tests: "includeTests",
	test: "includeTests",
	benches: "includeBenches",
	bench: "includeBenches",
	examples: "includeExamples",
	example: "includeExamples",
};

export function snapshotFromTree(input: {
	generation: number | null;
	prefix: string;
	tree: IdentityTree;
	edges: IdentityEdge[];
	config?: Partial<LayoutConfig>;
}): SceneSnapshot {
	const config: LayoutConfig = { ...DEFAULT_LAYOUT, ...input.config };
	const tree = filterTree(input.tree, config, 0);
	const laid = layoutCity(tree, input.edges, config);
	return {
		kind: "urban-plan",
		version: 3,
		generation: input.generation,
		prefix: input.prefix,
		capturedAt: new Date().toISOString(),
		heightMetric: "defs",
		footprintMetric: "defs",
		tree: input.tree,
		edges: input.edges,
		config,
		...laid,
	};
}

export function layoutCity(
	tree: IdentityTree,
	edges: IdentityEdge[],
	config: LayoutConfig,
): Pick<SceneSnapshot, "city" | "districts" | "buildings" | "streets" | "roads" | "coverage"> {
	const districts: SceneDistrict[] = [];
	const buildings: SceneBuilding[] = [];
	const streets: SceneStreet[] = [];
	const rootRect: Rect = { x: 0, z: 0, w: config.citySize, d: config.citySize };
	layoutNode({
		node: tree,
		rect: rootRect,
		level: 0,
		parentId: null,
		crateId: tree.id,
		crateColor: "#3d4a55",
		config,
		edges,
		districts,
		buildings,
		streets,
	});

	const crateCenters = new Map<string, { x: number; z: number }>();
	for (const district of districts) {
		if (district.level === 1) {
			crateCenters.set(district.id, { x: district.x, z: district.z });
		}
	}
	const crates = districts.filter((district) => district.level === 1);
	const ranked = [...edges]
		.filter(
			(edge) =>
				edge.count >= config.minRoadCount &&
				crateCenters.has(edge.source) &&
				crateCenters.has(edge.target),
		)
		.sort((a, b) => b.count - a.count);
	const roads: SceneRoad[] = config.showAvenues
		? routeAlongStreets({
				edges: ranked.slice(0, config.maxRoads).map((edge) => ({
					from: edge.source,
					to: edge.target,
					count: edge.count,
					kinds: edge.kinds,
				})),
				crates,
				districts,
				citySize: config.citySize,
			})
		: [];

	return {
		city: { width: config.citySize, depth: config.citySize },
		districts: config.showDistricts ? districts : [],
		buildings,
		streets: config.showStreets ? streets : [],
		roads,
		coverage: {
			nodes: buildings.length,
			districts: districts.length,
			edgesTotal: ranked.length,
			edgesEmitted: roads.length,
			roadsOmitted: Math.max(0, ranked.length - roads.length),
		},
	};
}

export function filterTree(node: IdentityTree, config: LayoutConfig, depth: number): IdentityTree {
	if (depth >= config.maxDepth) {
		return { ...node, children: [] };
	}
	const children = node.children
		.filter((child) => {
			if (depth === 0 && config.hiddenIds.includes(child.id)) {
				return false;
			}
			const flag = FOLDER_FLAGS[child.name];
			if (flag && !config[flag]) {
				return false;
			}
			if (child.children.length === 0) {
				if (child.defs < config.minDefs) {
					return false;
				}
				if (!config.buildingKinds.includes(child.kind) && child.kind !== "block") {
					return false;
				}
			}
			return true;
		})
		.map((child) => filterTree(child, config, depth + 1));
	return { ...node, children };
}

export function crateOptions(tree: IdentityTree): { id: string; name: string }[] {
	return tree.children.map((child) => ({ id: child.id, name: child.name }));
}

function gapAt(config: LayoutConfig, level: number): number {
	if (level <= 0) {
		return config.avenueGap;
	}
	if (level === 1) {
		return config.streetGap;
	}
	return config.laneGap;
}

function metricHeight(defs: number, config: LayoutConfig): number {
	const raw =
		config.heightMode === "linear" ? defs * config.heightScale * 0.02 : Math.log1p(defs) * config.heightScale;
	return Math.max(0.35, Math.min(config.maxHeight, raw));
}

function layoutNode(input: {
	node: IdentityTree;
	rect: Rect;
	level: number;
	parentId: string | null;
	crateId: string;
	crateColor: string;
	config: LayoutConfig;
	edges: IdentityEdge[];
	districts: SceneDistrict[];
	buildings: SceneBuilding[];
	streets: SceneStreet[];
}): void {
	const { node, rect, level, parentId, crateId, crateColor, config, edges, districts, buildings, streets } =
		input;
	const gap = gapAt(config, level);
	const inner = inset(rect, level === 0 ? Math.max(2.4, config.avenueGap * 0.85) : Math.min(gap * 0.28, 0.35));
	if (level > 1 && Math.min(rect.w, rect.d) < config.minDistrictSide) {
		packBuildings(flattenLeaves(node), inner, level, crateId, crateId, config, buildings, districts);
		return;
	}
	const thickness = 0.16 + Math.max(0, 3 - level) * 0.025;
	const districtY = thickness / 2 + Math.max(level - 1, 0) * 0.055;
	const clazz: RoadClass = level === 0 ? "avenue" : level === 1 ? "street" : "lane";
	if (level > 0) {
		districts.push({
			id: node.id,
			label: node.name,
			kind: node.kind,
			level,
			parentId,
			crateId,
			defs: node.defs,
			x: rect.x + rect.w / 2,
			y: districtY,
			z: rect.z + rect.d / 2,
			width: rect.w,
			depth: rect.d,
			thickness,
			color: shade(crateColor, 1 + Math.min(level - 1, 3) * 0.2),
		});
	}
	streets.push({
		id: `${node.id}::street`,
		clazz,
		x: inner.x + inner.w / 2,
		y: level === 0 ? 0.012 : districtY + thickness / 2 + 0.006,
		z: inner.z + inner.d / 2,
		width: inner.w,
		depth: inner.d,
	});

	if (node.children.length === 0) {
		placeBuilding(node, inner, level, node.id, crateId, config, buildings, districts);
		return;
	}

	const nested = node.children.filter((child) => child.children.length > 0);
	const leaves = node.children.filter((child) => child.children.length === 0);

	if (nested.length === 0) {
		packBuildings(leaves, inner, level, node.id, crateId, config, buildings, districts);
		return;
	}

	const orderedNested = level === 0 ? seriate(nested, edges) : nested;
	const items = [
		...orderedNested.map((child) => ({ node: child, area: areaOf(child) })),
		...(leaves.length > 0
			? [
					{
						node: packLeaves(node.id, leaves),
						area: Math.max(
							leaves.reduce((sum, leaf) => sum + Math.max(leaf.defs, 1), 0),
							1,
						),
					},
				]
			: []),
	];
	const cells =
		level === 0
			? orderedStrip(items, inner)
			: partition(
					items,
					inner,
					level === 1 ? config.minCrateSide * 0.36 : config.minDistrictSide + 0.7,
				);
	const childGap = gapAt(config, level);
	let crateIndex = 0;
	for (const cell of cells) {
		const childRect = inset(
			cell.rect,
			Math.min(childGap / 2, Math.min(cell.rect.w, cell.rect.d) * 0.12),
		);
		if (cell.node.id.endsWith("::block")) {
			packBuildings(cell.node.children, childRect, level + 1, node.id, crateId, config, buildings, districts);
			continue;
		}
		const nextCrateId = level === 0 ? cell.node.id : crateId;
		const nextColor = level === 0 ? CRATE_COLORS[crateIndex++ % CRATE_COLORS.length]! : crateColor;
		layoutNode({
			node: cell.node,
			rect: childRect,
			level: level + 1,
			parentId: level === 0 ? null : node.id,
			crateId: nextCrateId,
			crateColor: nextColor,
			config,
			edges,
			districts,
			buildings,
			streets,
		});
	}
}

function seriate(nodes: IdentityTree[], edges: IdentityEdge[]): IdentityTree[] {
	if (nodes.length <= 2) {
		return nodes;
	}
	const ids = nodes.map((node) => node.id);
	const weight = (a: string, b: string) =>
		edges
			.filter(
				(edge) =>
					(edge.source === a && edge.target === b) || (edge.source === b && edge.target === a),
			)
			.reduce((sum, edge) => sum + edge.count, 0);
	const strength = (id: string) => ids.reduce((sum, other) => sum + (other === id ? 0 : weight(id, other)), 0);
	const unused = new Set(ids);
	const start = [...ids].sort((a, b) => strength(b) - strength(a) || a.localeCompare(b))[0]!;
	const order = [start];
	unused.delete(start);
	while (unused.size > 0) {
		const last = order[order.length - 1]!;
		const next = [...unused].sort(
			(a, b) =>
				weight(b, last) + 0.25 * strength(b) - (weight(a, last) + 0.25 * strength(a)) || a.localeCompare(b),
		)[0]!;
		order.push(next);
		unused.delete(next);
	}
	const costOf = (ord: string[]) => {
		const pos = new Map(ord.map((id, index) => [id, index]));
		return edges.reduce((sum, edge) => {
			const i = pos.get(edge.source);
			const j = pos.get(edge.target);
			if (i === undefined || j === undefined) {
				return sum;
			}
			return sum + edge.count * Math.abs(i - j);
		}, 0);
	};
	let best = order;
	let bestCost = costOf(best);
	let changed = true;
	while (changed) {
		changed = false;
		for (let i = 0; i < best.length - 1; i += 1) {
			for (let j = i + 2; j <= best.length; j += 1) {
				const trial = [...best.slice(0, i), ...best.slice(i, j).reverse(), ...best.slice(j)];
				const cost = costOf(trial);
				if (cost < bestCost) {
					best = trial;
					bestCost = cost;
					changed = true;
				}
			}
		}
	}
	const byId = new Map(nodes.map((node) => [node.id, node]));
	return best.map((id) => byId.get(id)!).filter(Boolean);
}

function orderedStrip(
	items: { node: IdentityTree; area: number }[],
	rect: Rect,
): { node: IdentityTree; rect: Rect }[] {
	if (items.length === 0) {
		return [];
	}
	if (items.length === 1) {
		return [{ node: items[0]!.node, rect }];
	}
	const cols = Math.max(2, Math.ceil(items.length / 2));
	const rows: { node: IdentityTree; area: number }[][] = [];
	for (let i = 0; i < items.length; i += cols) {
		rows.push(items.slice(i, i + cols));
	}
	const rowAreas = rows.map((row) => row.reduce((sum, item) => sum + item.area, 0) || 1);
	const total = rowAreas.reduce((sum, area) => sum + area, 0) || 1;
	let depths = rowAreas.map((area) => rect.d * (area / total));
	const minDepth = rect.d * 0.16;
	depths = depths.map((depth) => Math.max(depth, minDepth));
	const depthSum = depths.reduce((sum, depth) => sum + depth, 0);
	depths = depths.map((depth) => (depth / depthSum) * rect.d);
	const placed: { node: IdentityTree; rect: Rect }[] = [];
	let z = rect.z;
	for (const [rowIndex, row] of rows.entries()) {
		const depth = depths[rowIndex]!;
		const rowArea = rowAreas[rowIndex]!;
		let x = rect.x;
		for (const item of row) {
			const width = rect.w * (item.area / rowArea);
			placed.push({ node: item.node, rect: { x, z, w: width, d: depth } });
			x += width;
		}
		z += depth;
	}
	return placed;
}

function areaOf(node: IdentityTree): number {
	if (node.children.length > 0) {
		return Math.max(
			node.children.reduce((sum, child) => sum + areaOf(child), 0),
			1,
		);
	}
	return Math.max(node.defs, 1);
}

function flattenLeaves(node: IdentityTree): IdentityTree[] {
	if (node.children.length === 0) {
		return [node];
	}
	return node.children.flatMap(flattenLeaves);
}

function packLeaves(parentId: string, leaves: IdentityTree[]): IdentityTree {
	return {
		id: `${parentId}::block`,
		name: "block",
		kind: "block",
		defs: leaves.reduce((sum, leaf) => sum + leaf.defs, 0),
		children: leaves,
	};
}

function packBuildings(
	leaves: IdentityTree[],
	rect: Rect,
	level: number,
	districtId: string,
	crateId: string,
	config: LayoutConfig,
	buildings: SceneBuilding[],
	districts: SceneDistrict[],
): void {
	if (leaves.length === 0) {
		return;
	}
	const sorted = [...leaves].sort((a, b) => b.defs - a.defs || a.name.localeCompare(b.name));
	const cols = Math.max(1, Math.ceil(Math.sqrt(sorted.length * (rect.w / Math.max(rect.d, 0.3)))));
	const rows = Math.max(1, Math.ceil(sorted.length / cols));
	const cellW = rect.w / cols;
	const cellD = rect.d / rows;
	const alley = Math.min(0.22, Math.min(cellW, cellD) * 0.18);
	const maxDefs = Math.max(1, ...sorted.map((leaf) => leaf.defs));
	const host = districts.find((item) => item.id === districtId);
	const platformTop = host ? host.y + host.thickness / 2 + 0.02 : 0.08 + Math.max(level - 1, 0) * 0.055;
	for (const [index, leaf] of sorted.entries()) {
		const col = index % cols;
		const row = Math.floor(index / cols);
		const t = Math.log1p(leaf.defs) / Math.log1p(maxDefs);
		const spanW = Math.max(0.35, Math.min(cellW - alley, (0.55 + t * 1.15) * config.footprintScale));
		const spanD = Math.max(0.35, Math.min(cellD - alley, (0.55 + t * 1.15) * config.footprintScale));
		const height = metricHeight(leaf.defs, config);
		buildings.push({
			id: leaf.id,
			label: leaf.name,
			kind: leaf.kind,
			districtId,
			crateId,
			defs: leaf.defs,
			x: rect.x + col * cellW + cellW / 2,
			y: platformTop + height / 2,
			z: rect.z + row * cellD + cellD / 2,
			width: spanW,
			depth: spanD,
			height,
		});
	}
}

function placeBuilding(
	node: IdentityTree,
	rect: Rect,
	level: number,
	districtId: string,
	crateId: string,
	config: LayoutConfig,
	buildings: SceneBuilding[],
	districts: SceneDistrict[],
): void {
	const host = districts.find((item) => item.id === districtId);
	const platformTop = host ? host.y + host.thickness / 2 + 0.02 : 0.1;
	const height = metricHeight(node.defs, config);
	const side = Math.max(0.4, Math.min(2.4, Math.min(rect.w, rect.d) * 0.62 * config.footprintScale));
	buildings.push({
		id: node.id,
		label: node.name,
		kind: node.kind,
		districtId,
		crateId,
		defs: node.defs,
		x: rect.x + rect.w / 2,
		y: platformTop + height / 2,
		z: rect.z + rect.d / 2,
		width: side,
		depth: side,
		height,
	});
}

function partition(
	items: { node: IdentityTree; area: number }[],
	rect: Rect,
	minSide: number,
): { node: IdentityTree; rect: Rect }[] {
	if (items.length === 0) {
		return [];
	}
	if (items.length === 1) {
		return [{ node: items[0]!.node, rect }];
	}
	const sorted = [...items].sort((a, b) => b.area - a.area);
	const total = sorted.reduce((sum, item) => sum + item.area, 0) || 1;
	let acc = 0;
	let splitAt = 0;
	const target = total / 2;
	while (splitAt < sorted.length - 1 && acc + sorted[splitAt]!.area <= target) {
		acc += sorted[splitAt]!.area;
		splitAt += 1;
	}
	if (splitAt === 0) {
		acc = sorted[0]!.area;
		splitAt = 1;
	}
	const left = sorted.slice(0, splitAt);
	const right = sorted.slice(splitAt);
	const alongX = rect.w >= rect.d;
	const long = alongX ? rect.w : rect.d;
	let share = (acc / total) * long;
	if (long >= minSide * 2) {
		share = Math.min(Math.max(share, minSide), long - minSide);
	}
	if (alongX) {
		return [
			...partition(left, { x: rect.x, z: rect.z, w: share, d: rect.d }, minSide),
			...partition(right, { x: rect.x + share, z: rect.z, w: rect.w - share, d: rect.d }, minSide),
		];
	}
	return [
		...partition(left, { x: rect.x, z: rect.z, w: rect.w, d: share }, minSide),
		...partition(right, { x: rect.x, z: rect.z + share, w: rect.w, d: rect.d - share }, minSide),
	];
}

function inset(rect: Rect, pad: number): Rect {
	const p = Math.max(0, Math.min(pad, rect.w / 2.6, rect.d / 2.6));
	return {
		x: rect.x + p,
		z: rect.z + p,
		w: Math.max(rect.w - p * 2, 0.35),
		d: Math.max(rect.d - p * 2, 0.35),
	};
}

function shade(hex: string, amount: number): string {
	const n = Number.parseInt(hex.slice(1), 16);
	const t = Math.max(0.35, Math.min(1.15, amount));
	const r = Math.min(255, Math.round(((n >> 16) & 255) * t));
	const g = Math.min(255, Math.round(((n >> 8) & 255) * t));
	const b = Math.min(255, Math.round((n & 255) * t));
	return `#${[r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}
