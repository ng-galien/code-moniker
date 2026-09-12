import type { SceneDistrict, ScenePoint, SceneRoad } from "./scene.ts";

/** Gutter centerline offset from crate faces. */
const CLEAR = 1.25;
const SNAP = 0.25;

type Box = { x0: number; z0: number; x1: number; z1: number };
type Edge = { to: number; w: number };

export function routeAlongStreets(input: {
	edges: { from: string; to: string; count: number; kinds: string[] }[];
	crates: SceneDistrict[];
	districts: SceneDistrict[];
	citySize: number;
}): SceneRoad[] {
	const boxes = input.crates.map(boxOf);
	const grid = hanan(input.crates, input.citySize, boxes);
	const slack = 6;
	return input.edges.map((edge) => {
		const from = input.crates.find((crate) => crate.id === edge.from);
		const to = input.crates.find((crate) => crate.id === edge.to);
		const points =
			from && to
				? align(pathFor(from, to, input.districts, grid, slack))
				: [];
		return {
			from: edge.from,
			to: edge.to,
			fromLabel: from?.label ?? edge.from,
			toLabel: to?.label ?? edge.to,
			count: edge.count,
			kinds: edge.kinds,
			clazz: "avenue",
			points,
		};
	});
}

function pathFor(
	from: SceneDistrict,
	to: SceneDistrict,
	districts: SceneDistrict[],
	grid: { nodes: ScenePoint[]; graph: Edge[][] },
	slack: number,
): ScenePoint[] {
	const shared = sharedStreet(from, to, slack);
	if (shared) {
		return crossing(from, to, shared, districts);
	}
	const g1 = gate(from, to);
	const g2 = gate(to, from);
	const via = walk(grid.nodes, grid.graph, nearest(grid.nodes, g1), nearest(grid.nodes, g2));
	const streetFrom = via[0] ?? g1;
	const streetTo = via[via.length - 1] ?? g2;
	const v1 = Math.abs(streetFrom.x - from.x) >= Math.abs(streetFrom.z - from.z);
	const v2 = Math.abs(streetTo.x - to.x) >= Math.abs(streetTo.z - to.z);
	return [enter(from, streetFrom, v1, districts), ...via, enter(to, streetTo, v2, districts)];
}

function crossing(
	from: SceneDistrict,
	to: SceneDistrict,
	shared: ScenePoint,
	districts: SceneDistrict[],
): ScenePoint[] {
	const vertical = Math.abs(from.x - to.x) >= Math.abs(from.z - to.z);
	return [
		enter(from, shared, vertical, districts),
		shared,
		enter(to, shared, vertical, districts),
	];
}

function enter(
	crate: SceneDistrict,
	street: ScenePoint,
	verticalStreet: boolean,
	districts: SceneDistrict[],
): ScenePoint {
	const src = districts.find(
		(district) => district.crateId === crate.id && district.level === 2 && district.label === "src",
	);
	const host = src ?? crate;
	const box = boxOf(host);
	if (verticalStreet) {
		const x = street.x <= host.x ? box.x0 + 1.1 : box.x1 - 1.1;
		return snapPoint({ x, z: clamp(street.z, box.z0 + 0.8, box.z1 - 0.8) });
	}
	const z = street.z <= host.z ? box.z0 + 1.1 : box.z1 - 1.1;
	return snapPoint({ x: clamp(street.x, box.x0 + 0.8, box.x1 - 0.8), z });
}

function port(crate: SceneDistrict, toward: SceneDistrict, districts: SceneDistrict[]): ScenePoint {
	const src = districts.find(
		(district) => district.crateId === crate.id && district.level === 2 && district.label === "src",
	);
	return onFace(src ?? crate, toward, -1.2);
}

function clamp(value: number, min: number, max: number): number {
	if (min > max) {
		return (min + max) / 2;
	}
	return Math.max(min, Math.min(max, value));
}

function sharedStreet(a: SceneDistrict, b: SceneDistrict, slack: number): ScenePoint | null {
	const A = boxOf(a);
	const B = boxOf(b);
	const overlapZ = Math.min(A.z1, B.z1) - Math.max(A.z0, B.z0);
	const overlapX = Math.min(A.x1, B.x1) - Math.max(A.x0, B.x0);
	if (overlapZ > Math.min(a.depth, b.depth) * 0.2) {
		const gap = A.x1 <= B.x0 ? B.x0 - A.x1 : A.x0 - B.x1;
		if (gap >= 0 && gap <= slack) {
			return snapPoint({
				x: A.x1 <= B.x0 ? (A.x1 + B.x0) / 2 : (B.x1 + A.x0) / 2,
				z: (Math.max(A.z0, B.z0) + Math.min(A.z1, B.z1)) / 2,
			});
		}
	}
	if (overlapX > Math.min(a.width, b.width) * 0.2) {
		const gap = A.z1 <= B.z0 ? B.z0 - A.z1 : A.z0 - B.z1;
		if (gap >= 0 && gap <= slack) {
			return snapPoint({
				x: (Math.max(A.x0, B.x0) + Math.min(A.x1, B.x1)) / 2,
				z: A.z1 <= B.z0 ? (A.z1 + B.z0) / 2 : (B.z1 + A.z0) / 2,
			});
		}
	}
	return null;
}

function boxOf(district: SceneDistrict): Box {
	return {
		x0: district.x - district.width / 2,
		z0: district.z - district.depth / 2,
		x1: district.x + district.width / 2,
		z1: district.z + district.depth / 2,
	};
}

function gate(from: SceneDistrict, toward: SceneDistrict): ScenePoint {
	return onFace(from, toward, CLEAR);
}

function dock(from: SceneDistrict, toward: SceneDistrict): ScenePoint {
	return onFace(from, toward, -1.15);
}

function onFace(from: SceneDistrict, toward: SceneDistrict, outward: number): ScenePoint {
	const dx = toward.x - from.x;
	const dz = toward.z - from.z;
	if (Math.abs(dx) >= Math.abs(dz)) {
		return snapPoint({
			x: from.x + Math.sign(dx || 1) * (from.width / 2 + outward),
			z: from.z,
		});
	}
	return snapPoint({
		x: from.x,
		z: from.z + Math.sign(dz || 1) * (from.depth / 2 + outward),
	});
}

function hanan(
	crates: SceneDistrict[],
	citySize: number,
	boxes: Box[],
): { nodes: ScenePoint[]; graph: Edge[][] } {
	const xs = new Set<number>([CLEAR, snap(citySize / 2), citySize - CLEAR]);
	const zs = new Set<number>([CLEAR, snap(citySize / 2), citySize - CLEAR]);
	for (const crate of crates) {
		const b = boxOf(crate);
		xs.add(snap(b.x0 - CLEAR));
		xs.add(snap(b.x1 + CLEAR));
		xs.add(snap(crate.x));
		zs.add(snap(b.z0 - CLEAR));
		zs.add(snap(b.z1 + CLEAR));
		zs.add(snap(crate.z));
	}
	const xList = [...xs].filter((x) => x >= 0 && x <= citySize).sort((a, b) => a - b);
	const zList = [...zs].filter((z) => z >= 0 && z <= citySize).sort((a, b) => a - b);
	const nodes: ScenePoint[] = [];
	const index = new Map<string, number>();
	for (const x of xList) {
		for (const z of zList) {
			if (boxes.some((box) => inside(box, x, z))) {
				continue;
			}
			index.set(`${x},${z}`, nodes.length);
			nodes.push({ x, z });
		}
	}
	const graph: Edge[][] = nodes.map(() => []);
	const link = (i: number, j: number) => {
		const a = nodes[i]!;
		const b = nodes[j]!;
		if (blocked(a, b, boxes)) {
			return;
		}
		const w = Math.abs(a.x - b.x) + Math.abs(a.z - b.z);
		graph[i]!.push({ to: j, w });
		graph[j]!.push({ to: i, w });
	};
	for (const z of zList) {
		const row = xList
			.map((x) => index.get(`${x},${z}`))
			.filter((i): i is number => i !== undefined);
		for (let k = 1; k < row.length; k += 1) {
			link(row[k - 1]!, row[k]!);
		}
	}
	for (const x of xList) {
		const col = zList
			.map((z) => index.get(`${x},${z}`))
			.filter((i): i is number => i !== undefined);
		for (let k = 1; k < col.length; k += 1) {
			link(col[k - 1]!, col[k]!);
		}
	}
	return { nodes, graph };
}

function walk(nodes: ScenePoint[], graph: Edge[][], start: number, goal: number): ScenePoint[] {
	if (start < 0 || goal < 0) {
		return [];
	}
	const dist = new Array<number>(nodes.length).fill(Infinity);
	const prev = new Array<number>(nodes.length).fill(-1);
	dist[start] = 0;
	const open = [start];
	while (open.length > 0) {
		let bestAt = 0;
		for (let i = 1; i < open.length; i += 1) {
			if (dist[open[i]!]! < dist[open[bestAt]!]!) {
				bestAt = i;
			}
		}
		const current = open.splice(bestAt, 1)[0]!;
		if (current === goal) {
			break;
		}
		for (const edge of graph[current] ?? []) {
			const next = dist[current]! + edge.w;
			if (next >= dist[edge.to]!) {
				continue;
			}
			dist[edge.to] = next;
			prev[edge.to] = current;
			open.push(edge.to);
		}
	}
	if (dist[goal] === Infinity) {
		return [];
	}
	const idx: number[] = [];
	for (let i = goal; i >= 0; i = prev[i]!) {
		idx.push(i);
		if (i === start) {
			break;
		}
	}
	idx.reverse();
	if (idx[0] !== start) {
		return [];
	}
	return idx.map((i) => nodes[i]!);
}

function nearest(nodes: ScenePoint[], point: ScenePoint): number {
	let best = -1;
	let bestD = Infinity;
	for (let i = 0; i < nodes.length; i += 1) {
		const d = Math.abs(nodes[i]!.x - point.x) + Math.abs(nodes[i]!.z - point.z);
		if (d < bestD) {
			bestD = d;
			best = i;
		}
	}
	return best;
}

function blocked(a: ScenePoint, b: ScenePoint, boxes: Box[]): boolean {
	const eps = 0.08;
	if (a.z === b.z) {
		const z = a.z;
		const x0 = Math.min(a.x, b.x);
		const x1 = Math.max(a.x, b.x);
		return boxes.some(
			(box) => z > box.z0 + eps && z < box.z1 - eps && x0 < box.x1 - eps && x1 > box.x0 + eps,
		);
	}
	if (a.x === b.x) {
		const x = a.x;
		const z0 = Math.min(a.z, b.z);
		const z1 = Math.max(a.z, b.z);
		return boxes.some(
			(box) => x > box.x0 + eps && x < box.x1 - eps && z0 < box.z1 - eps && z1 > box.z0 + eps,
		);
	}
	return true;
}

function inside(box: Box, x: number, z: number): boolean {
	return x > box.x0 + 0.04 && x < box.x1 - 0.04 && z > box.z0 + 0.04 && z < box.z1 - 0.04;
}

function align(points: ScenePoint[]): ScenePoint[] {
	if (points.length <= 2) {
		return points;
	}
	const out: ScenePoint[] = [points[0]!];
	for (let i = 1; i < points.length - 1; i += 1) {
		const prev = out[out.length - 1]!;
		const cur = points[i]!;
		const next = points[i + 1]!;
		const col = (prev.x === cur.x && cur.x === next.x) || (prev.z === cur.z && cur.z === next.z);
		if (!col) {
			out.push(cur);
		}
	}
	out.push(points[points.length - 1]!);
	return out;
}

function snapPoint(point: ScenePoint): ScenePoint {
	return { x: snap(point.x), z: snap(point.z) };
}

function snap(value: number): number {
	return Math.round(value / SNAP) * SNAP;
}
